use crate::parser;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path;

#[derive(Deserialize, Serialize)]
pub struct RubyBox {
    #[serde(with = "regex_array")]
    pub imports: Vec<Regex>,
    #[serde(with = "regex_array")]
    pub exports: Vec<Regex>,
}

mod regex_array {
    use regex::Regex;
    use serde::{self, ser::SerializeSeq, Deserialize, Deserializer, Serializer};

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Regex>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let mut res = Vec::new();
        let v: Vec<String> = Vec::deserialize(deserializer)?;
        for pattern in v {
            res.push(Regex::new(&pattern).unwrap());
        }
        Ok(res)
    }

    pub fn serialize<S>(regs: &Vec<Regex>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(regs.len()))?;
        for reg in regs {
            seq.serialize_element(reg.as_str())?;
        }
        seq.end()
    }
}

pub enum ViolationDirection {
    NonImportedReference,
    NonExportedReference,
}

pub struct BoxViolation {
    pub dir: ViolationDirection,
    pub rel: parser::Relation,
}

impl std::fmt::Display for BoxViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.dir {
            ViolationDirection::NonImportedReference => write!(
                f,
                "non imported reference {} found in {} on line {}",
                self.rel.namespace,
                self.rel.file.to_str().unwrap(),
                self.rel.line
            ),
            ViolationDirection::NonExportedReference => write!(
                f,
                "non exported reference {} found in {} on line {}",
                self.rel.namespace,
                self.rel.file.to_str().unwrap(),
                self.rel.line
            ),
        }
    }
}

pub fn parse(s: &str) -> Result<RubyBox, serde_yaml::Error> {
    let b: RubyBox = match serde_yaml::from_str(&s) {
        Ok(b) => b,
        Err(_) => RubyBox {
            imports: Vec::new(),
            exports: Vec::new(),
        },
    };
    Ok(b)
}

pub fn enforce_box<'a>(
    box_path: &'a path::PathBuf,
    ruby_box: RubyBox,
    defs: &'a [parser::Definition],
    rels: &'a [parser::Relation],
    ignores: &'a [glob::Pattern],
) -> Vec<BoxViolation> {
    let mut violations: Vec<BoxViolation> = Vec::new();
    let box_dir_opt = box_path.parent();
    let box_dir = match box_dir_opt {
        None => return violations,
        Some(d) => d,
    };

    let ref defs_in_box: Vec<&parser::Definition> = defs
        .iter()
        .filter(|d| d.file.starts_with(box_dir))
        .collect();

    let rels_not_exported: Vec<&parser::Relation> = rels
        .iter()
        .filter(|r| {
            ignores.iter().all(|g| {
                if let Some(s) = r.file.to_str() {
                    !g.matches(s)
                } else {
                    true
                }
            })
        })
        .filter(|r| {
            defs_in_box.iter().any(|d| d.namespace == r.namespace)
                && !ruby_box.exports.iter().any(|b| b.is_match(&r.namespace))
        })
        .collect();

    for rel in rels_not_exported {
        violations.push(BoxViolation {
            rel: rel.clone(),
            dir: ViolationDirection::NonExportedReference,
        })
    }

    let rels_inside_box_not_imported: Vec<&parser::Relation> = rels
        .iter()
        .filter(|r| {
            r.file.starts_with(box_dir)
                && !(ruby_box.imports.iter().any(|b| b.is_match(&r.namespace))
                    || matches_to_self(r, defs_in_box))
        })
        .collect();

    for rel in rels_inside_box_not_imported {
        violations.push(BoxViolation {
            rel: rel.clone(),
            dir: ViolationDirection::NonImportedReference,
        })
    }

    violations
}

fn matches_to_self(rel: &parser::Relation, defs: &Vec<&parser::Definition>) -> bool {
    let mut parts: Vec<&str> = rel.caller_namespace.split("::").collect();
    parts.pop();
    parts.push(&rel.namespace);
    let ns1 = parts.join("::");

    let mut parts: Vec<&str> = rel.caller_namespace.split("::").collect();
    parts.push(&rel.namespace);
    let ns2 = parts.join("::");

    let mut parts: Vec<&str> = rel.caller_namespace.split("::").collect();
    parts.pop();
    parts.pop();
    parts.push(&rel.namespace);
    let ns3 = parts.join("::");

    let mut parts: Vec<&str> = rel.caller_namespace.split("::").collect();
    parts.pop();
    parts.pop();
    parts.pop();
    parts.push(&rel.namespace);
    let ns4 = parts.join("::");

    defs.iter().any(|d| {
        d.namespace == rel.namespace
            || d.namespace == ns1
            || d.namespace == ns2
            || d.namespace == ns3
            || d.namespace == ns4
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    struct BoxConstraintTest {
        name: String,
        box_path: path::PathBuf,
        ruby_box: RubyBox,
        ignores: Vec<glob::Pattern>,
        defs: Vec<parser::Definition>,
        rels: Vec<parser::Relation>,
        violations: Vec<BoxViolation>,
    }

    impl BoxConstraintTest {
        pub fn new(name: &str, box_path: &str) -> BoxConstraintTest {
            BoxConstraintTest {
                name: String::from(name),
                box_path: path::PathBuf::from(box_path),
                ruby_box: RubyBox {
                    imports: Vec::new(),
                    exports: Vec::new(),
                },
                ignores: Vec::new(),
                defs: Vec::new(),
                rels: Vec::new(),
                violations: Vec::new(),
            }
        }
    }

    impl parser::Relation {
        pub fn new(caller_namespace: &str, namespace: &str, file: &str) -> parser::Relation {
            parser::Relation {
                caller_namespace: String::from(caller_namespace),
                namespace: String::from(namespace),
                file: path::PathBuf::from(file),
                line: 0,
            }
        }
    }

    impl parser::Definition {
        pub fn new(namespace: &str, file: &str) -> parser::Definition {
            parser::Definition {
                namespace: String::from(namespace),
                file: path::PathBuf::from(file),
                line: 0,
                lines: 0,
            }
        }
    }

    #[test]
    fn enforces_box_constraints() {
        let mut tests: Vec<BoxConstraintTest> = Vec::new();
        {
            let mut test = BoxConstraintTest::new("single external reference", "lib/mod/box.yaml");
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.rels
                .push(parser::Relation::new("A", "Z", "lib/mod/mod.rb"));
            test.violations.push(BoxViolation {
                dir: ViolationDirection::NonImportedReference,
                rel: test.rels[0].clone(),
            });
            tests.push(test);
        }
        {
            let mut test = BoxConstraintTest::new(
                "single external reference defined import",
                "lib/mod/box.yaml",
            );
            test.ruby_box.imports.push(Regex::from_str("Z").unwrap());
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.rels
                .push(parser::Relation::new("A", "Z", "lib/mod/mod.rb"));
            tests.push(test);
        }
        {
            let mut test = BoxConstraintTest::new("single incoming reference", "lib/mod/box.yaml");
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.defs
                .push(parser::Definition::new("B", "lib/mod2/mod.rb"));
            test.rels
                .push(parser::Relation::new("B", "A", "lib/mod2/mod.rb"));
            test.violations.push(BoxViolation {
                dir: ViolationDirection::NonExportedReference,
                rel: test.rels[0].clone(),
            });
            tests.push(test);
        }
        {
            let mut test = BoxConstraintTest::new("single incoming reference", "lib/mod/box.yaml");
            test.ruby_box.exports.push(Regex::from_str("A").unwrap());
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.defs
                .push(parser::Definition::new("B", "lib/mod2/mod.rb"));
            test.rels
                .push(parser::Relation::new("B", "A", "lib/mod2/mod.rb"));
            tests.push(test);
        }
        {
            let mut test = BoxConstraintTest::new("internal reference ok", "lib/mod/box.yaml");
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.defs
                .push(parser::Definition::new("A::B", "lib/mod/mod.rb"));
            test.rels
                .push(parser::Relation::new("A", "B", "lib/mod/mod.rb"));
            tests.push(test);
        }
        {
            let mut test = BoxConstraintTest::new("respect ignores", "lib/mod/box.yaml");
            test.ignores
                .push(glob::Pattern::new("lib/mod2/*.*").unwrap());
            test.defs
                .push(parser::Definition::new("A", "lib/mod/mod.rb"));
            test.defs
                .push(parser::Definition::new("B", "lib/mod2/mod.rb"));
            test.rels
                .push(parser::Relation::new("B", "A", "lib/mod2/mod.rb"));
            tests.push(test);
        }
        for test in tests {
            let results = enforce_box(
                &test.box_path,
                test.ruby_box,
                &test.defs,
                &test.rels,
                &test.ignores,
            );
            assert_eq!(
                results.len(),
                test.violations.len(),
                "{}: expected {} results but found {}",
                test.name,
                test.violations.len(),
                results.len()
            );
            for v in test.violations {
                assert_eq!(
                    results.iter().any(|r| r.rel == v.rel),
                    true,
                    "{}: expected to find {} but did not",
                    test.name,
                    v.rel.namespace
                );
            }
        }
    }
}
