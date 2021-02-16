use crossbeam_channel::{Receiver, Sender};
use lib_ruby_parser::traverse::Visitor;
use lib_ruby_parser::{Node, Parser, ParserOptions, ParserResult};
use std::cell::Cell;
use std::fs::File;
use std::io::Read;
use std::option::Option;
use std::path;
mod constants;

pub struct Definition {
    pub namespace: String,
    pub file: path::PathBuf,
    pub line: usize,
    pub lines: usize,
}

#[derive(PartialEq, Clone)]
pub struct Relation {
    pub namespace: String,
    pub file: path::PathBuf,
    pub line: usize,
    pub caller_namespace: String,
}

pub struct RubyFile {
    pub definitions: Vec<Definition>,
    pub relations: Vec<Relation>,
}

struct ExtractConsts<'a> {
    file: path::PathBuf,
    parents: Vec<String>,
    ruby_file: RubyFile,
    parser_result: &'a ParserResult,
}

impl<'a> lib_ruby_parser::traverse::Visitor<Option<Node>> for ExtractConsts<'a> {
    fn on_module(&mut self, node: &lib_ruby_parser::nodes::Module) -> Option<Node> {
        let ns = namespace(node.name.as_ref().clone(), &mut self.parents.clone()).unwrap();
        let def = Definition {
            namespace: ns,
            file: self.file.clone(),
            line: node.keyword_l.begin_pos,
            lines: node.end_l.end_pos,
        };
        self.ruby_file.definitions.push(def);

        let ns = extract_name(node.name.as_ref().clone()).unwrap();
        self.parents.push(ns);
        let resp = self.maybe_visit(&node.body);
        self.parents.pop();
        resp
    }

    fn on_class(&mut self, node: &lib_ruby_parser::nodes::Class) -> Option<Node> {
        let ns = namespace(node.name.as_ref().clone(), &mut self.parents.clone()).unwrap();
        let def = Definition {
            namespace: ns,
            file: self.file.clone(),
            line: node.keyword_l.begin_pos,
            lines: node.end_l.end_pos,
        };

        self.ruby_file.definitions.push(def);

        let ns = extract_name(node.name.as_ref().clone()).unwrap();
        self.parents.push(ns);
        self.maybe_visit(&node.superclass);
        let resp = self.maybe_visit(&node.body);
        self.parents.pop();
        resp
    }

    fn on_casgn(&mut self, node: &lib_ruby_parser::nodes::Casgn) -> Option<Node> {
        let mut ns = self.parents.clone();
        let scope = Cell::new(node.scope.clone());
        while let Some(b) = scope.take() {
            if let Node::Const(n) = *b {
                ns.push(n.name);
                scope.set(n.scope);
            }
        }
        ns.push(node.name.clone());
        let def = Definition {
            namespace: ns.join("::"),
            file: self.file.clone(),
            line: node.name_l.begin_pos,
            lines: node.name_l.size(),
        };

        self.ruby_file.definitions.push(def);

        None
    }

    fn on_const(&mut self, node: &lib_ruby_parser::nodes::Const) -> Option<Node> {
        let mut ns = Vec::new();
        let scope = Cell::new(node.scope.clone());
        while let Some(b) = scope.take() {
            if let Node::Const(n) = *b {
                ns.push(n.name);
                scope.set(n.scope);
            }
        }
        ns.reverse();
        ns.push(node.name.clone());
        let full_ns = ns.join("::");
        if constants::RUBY.contains(&full_ns.as_str()) {
            return None;
        }
        let (line, _) = node
            .expression_l
            .expand_to_line(&self.parser_result.input)
            .unwrap();
        let rel = Relation {
            namespace: full_ns,
            caller_namespace: self.parents.join("::"),
            file: self.file.clone(),
            line: line + 1,
        };
        self.ruby_file.relations.push(rel);

        None
    }
}

#[derive(Debug)]
struct NotAConstError(Node);

impl std::fmt::Display for NotAConstError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "Assumed name was const but it wasnt: {}",
            self.0.str_type()
        )
    }
}

impl std::error::Error for NotAConstError {}

fn namespace(name: Node, parents: &mut Vec<String>) -> Result<String, Box<dyn std::error::Error>> {
    let n = extract_name(name)?;
    parents.push(n);
    Ok(parents.join("::"))
}

fn extract_name(name: Node) -> Result<String, Box<dyn std::error::Error>> {
    match name {
        Node::Const(c) => Ok(c.name),
        _ => Err(Box::new(NotAConstError(name))),
    }
}

fn ruby_file(path: path::PathBuf, contents: &[u8]) -> Result<RubyFile, Box<dyn std::error::Error>> {
    let options = ParserOptions {
        buffer_name: "(eval)".to_owned(),
        debug: false,
        ..Default::default()
    };
    let parser = Parser::new(&contents, options);
    let result = parser.do_parse();
    let ruby_file = RubyFile {
        definitions: Vec::new(),
        relations: Vec::new(),
    };
    let mut visitor = ExtractConsts {
        parents: Vec::new(),
        file: path.clone(),
        ruby_file: ruby_file,
        parser_result: &result,
    };

    match &result.ast {
        Some(n) => visitor.visit(&n),
        None => {
            return Ok(RubyFile {
                definitions: Vec::new(),
                relations: Vec::new(),
            })
        }
    };

    let mut defs = Vec::new();
    let mut rels = Vec::new();

    for def in visitor.ruby_file.definitions.drain(0..) {
        defs.push(def);
    }

    for rel in visitor.ruby_file.relations.drain(0..) {
        rels.push(rel);
    }

    Ok(RubyFile {
        definitions: defs,
        relations: rels,
    })
}

pub fn worker(
    rx: Receiver<path::PathBuf>,
    tx: Sender<RubyFile>,
) -> Result<(), Box<dyn std::error::Error>> {
    for path in rx.iter() {
        let mut file = File::open(&path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        let rf = ruby_file(path, &contents)?;
        tx.send(rf)?;
    }

    Ok(())
}
