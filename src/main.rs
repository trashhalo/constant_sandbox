use clap::{value_t_or_exit, values_t, App, Arg, SubCommand};

use crossbeam_channel::bounded;
use glob::glob;
use regex::Regex;
use std::collections::HashSet;
use std::fs::File;
use std::io::prelude::*;
use std::io::Read;
use std::path;
use std::str;
use std::thread;
mod parser;
mod ruby_box;

fn parse_ruby(
) -> Result<(Vec<parser::Definition>, Vec<parser::Relation>), Box<dyn std::error::Error>> {
    let cpus = num_cpus::get();

    let (work_tx, work_rx) = bounded(0);
    let (collect_tx, collect_rx) = bounded(0);
    let (results_tx, results_rx) = bounded(0);

    let mut children = Vec::new();
    for _id in 0..cpus {
        let thread_work_rx = work_rx.clone();
        let thread_results_tx = collect_tx.clone();
        let child = thread::spawn(move || {
            parser::worker(thread_work_rx, thread_results_tx).unwrap();
        });
        children.push(child);
    }

    let thread_results_rx = collect_rx.clone();
    let results_collector = thread::spawn(move || {
        let mut results = Vec::new();
        for result in thread_results_rx.iter() {
            results.push(result);
        }
        results_tx.send(results).unwrap();
        drop(results_tx);
    });

    for entry in glob("**/*.rb").expect("Failed to read glob pattern") {
        let path = entry?;
        work_tx.send(path)?;
    }

    drop(work_tx);

    for child in children {
        child.join().expect("the child thread panicked");
    }

    drop(collect_tx);

    let results = results_rx.recv()?;
    results_collector
        .join()
        .expect("results collector panicked");

    let mut defs = Vec::new();
    let mut rels = Vec::new();
    for mut result in results {
        for def in result.definitions.drain(0..) {
            defs.push(def);
        }
        for rel in result.relations.drain(0..) {
            rels.push(rel);
        }
    }
    Ok((defs, rels))
}

enum Command<'a> {
    Init(&'a clap::ArgMatches<'a>),
    Inspect(&'a clap::ArgMatches<'a>),
    Verify(&'a clap::ArgMatches<'a>),
}

fn subcommand<'a>(app: &'a clap::ArgMatches) -> Result<Command<'a>, Box<dyn std::error::Error>> {
    match app.subcommand() {
        ("init", Some(m)) => Ok(Command::Init(m)),
        ("inspect", Some(m)) => Ok(Command::Inspect(m)),
        ("verify", Some(m)) => Ok(Command::Verify(m)),
        (_, None) => Ok(Command::Verify(app)),
        (_, Some(_)) => Err("recieved a unknown subcommand".into()),
    }
}

fn command_init(matches: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let (defs, rels) = parse_ruby().unwrap();
    let box_str = value_t_or_exit!(matches.value_of("box"), String);
    let ref path: path::PathBuf = box_str.into();
    if path.exists() {
        std::fs::remove_file(path)?;
    }

    let rb = ruby_box::RubyBox {
        imports: Vec::new(),
        exports: Vec::new(),
    };
    let ignores: Result<Vec<glob::Pattern>, glob::PatternError> =
        if let Ok(values) = values_t!(matches.values_of("ignore"), String) {
            values.into_iter().map(|v| glob::Pattern::new(&v)).collect()
        } else {
            Ok(Vec::new())
        };
    let ref errors = ruby_box::enforce_box(path, rb, &defs, &rels, &ignores?);
    println!("updating box {:?}", path);
    let mut exports = HashSet::new();
    let mut imports = HashSet::new();
    for error in errors {
        match error.dir {
            ruby_box::ViolationDirection::NonImportedReference => {
                imports.insert(error.rel.namespace.clone());
            }
            ruby_box::ViolationDirection::NonExportedReference => {
                exports.insert(error.rel.namespace.clone());
            }
        };
    }

    let mut exports_vec: Vec<String> = exports.drain().collect();
    exports_vec.sort();
    let exports_vec = exports_vec
        .iter()
        .map(|s| Regex::new(&s).unwrap())
        .collect();

    let mut imports_vec: Vec<String> = imports.drain().collect();
    imports_vec.sort();
    let imports_vec = imports_vec
        .iter()
        .map(|s| Regex::new(&s).unwrap())
        .collect();

    let yaml = serde_yaml::to_string(&ruby_box::RubyBox {
        exports: exports_vec,
        imports: imports_vec,
    })?;

    let mut file = File::create(path)?;
    file.write_all(yaml.as_bytes())?;
    Ok(())
}

fn command_inspect(matches: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let (defs, rels) = parse_ruby().unwrap();
    let box_str = value_t_or_exit!(matches.value_of("box"), String);
    let mut path: path::PathBuf = box_str.into();
    path = path.join("box.yml");

    let rb = ruby_box::RubyBox {
        imports: Vec::new(),
        exports: Vec::new(),
    };
    let ignores: Result<Vec<glob::Pattern>, glob::PatternError> =
        if let Ok(values) = values_t!(matches.values_of("ignore"), String) {
            values.into_iter().map(|v| glob::Pattern::new(&v)).collect()
        } else {
            Ok(Vec::new())
        };
    let ref errors = ruby_box::enforce_box(&path, rb, &defs, &rels, &ignores?);
    let mut exports = HashSet::new();
    let mut imports = HashSet::new();
    for error in errors {
        println!("{}", error);
        match error.dir {
            ruby_box::ViolationDirection::NonImportedReference => {
                imports.insert(error.rel.namespace.clone());
            }
            ruby_box::ViolationDirection::NonExportedReference => {
                exports.insert(error.rel.namespace.clone());
            }
        };
    }

    let mut exports_vec: Vec<String> = exports.drain().collect();
    exports_vec.sort();
    let exports_vec = exports_vec
        .iter()
        .map(|s| Regex::new(&s).unwrap())
        .collect();

    let mut imports_vec: Vec<String> = imports.drain().collect();
    imports_vec.sort();
    let imports_vec = imports_vec
        .iter()
        .map(|s| Regex::new(&s).unwrap())
        .collect();

    let yaml = serde_yaml::to_string(&ruby_box::RubyBox {
        exports: exports_vec,
        imports: imports_vec,
    })?;

    println!("{}", yaml);
    Ok(())
}

fn command_verify(matches: &clap::ArgMatches) -> Result<(), Box<dyn std::error::Error>> {
    let (defs, rels) = parse_ruby().unwrap();
    let mut has_errors = false;

    for entry in glob("**/box.yml").expect("Failed to read glob pattern") {
        let path = entry?;
        let mut file = File::open(&path)?;
        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;
        let rb = ruby_box::parse(str::from_utf8(&contents)?)?;
        let ignores: Result<Vec<glob::Pattern>, glob::PatternError> =
            if let Ok(values) = values_t!(matches.values_of("ignore"), String) {
                values.into_iter().map(|v| glob::Pattern::new(&v)).collect()
            } else {
                Ok(Vec::new())
            };
        let ref errors = ruby_box::enforce_box(&path, rb, &defs, &rels, &ignores?);
        println!("verifing box {:?}", path);
        for error in errors {
            println!("{}", error);
        }
        if errors.len() > 0 && !has_errors {
            has_errors = true;
        }
    }
    if has_errors {
        Err("found box violations".into())
    } else {
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let matches = App::new("constant_sandbox")
        .version("1.0")
        .author("Stephen Solka <solka@hey.com>")
        .about("Control the constants that leak in and out of an area of your codebase.")
        .subcommand(
            SubCommand::with_name("init")
                .about("Generate a box at a location in your codebase")
                .arg(
                    Arg::with_name("box")
                        .help("location to generate a box")
                        .index(1),
                )
                .arg(
                    Arg::with_name("ignore")
                        .short("i")
                        .help("glob of tiles to ignore")
                        .takes_value(true)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("inspect")
                .about("Inspect an area of a codebase for imports and exports. Read only.")
                .arg(
                    Arg::with_name("box")
                        .help("location to inspect")
                        .index(1)
                        .required(true),
                )
                .arg(
                    Arg::with_name("ignore")
                        .short("i")
                        .help("glob of tiles to ignore")
                        .takes_value(true)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("verify")
                .about("Verify boxed areas of a codebase comply with defined imports and exports.")
                .arg(
                    Arg::with_name("ignore")
                        .short("i")
                        .help("glob of tiles to ignore")
                        .takes_value(true)
                        .multiple(true),
                ),
        )
        .get_matches();

    match subcommand(&matches) {
        Ok(Command::Init(matches)) => command_init(matches),
        Ok(Command::Inspect(matches)) => command_inspect(matches),
        Ok(Command::Verify(matches)) => command_verify(matches),
        Err(e) => Err(e),
    }
}
