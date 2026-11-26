//! A suite's coverage as IntelliJ's engines give it (`ProjectData`): each class's lines with
//! their hits and branches, and the tests that covered them; and IntelliJ's counts over it
//! (`PackageAnnotator`): classes, methods, lines and branches, covered of all.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// How a line is covered (`LineCoverage`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    #[default]
    None,
    Partial,
    Full,
}

/// A conditional jump: how often the condition held and how often it didn't.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Jump {
    pub true_hits: u32,
    pub false_hits: u32,
}

/// A switch: each case's key and hits, and the default's hits.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Switch {
    pub keys: Vec<i32>,
    pub hits: Vec<u32>,
    pub default_hits: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Line {
    /// From 1.
    pub line: u32,
    pub hits: u32,
    /// The method's name and JVM descriptor (`sign(I)Ljava/lang/String;`).
    pub method: String,
    pub jumps: Vec<Jump>,
    pub switches: Vec<Switch>,
}

impl Line {
    /// IntelliJ's `LineData.getStatus`: not hit, hit with a branch never taken, or hit fully.
    pub fn status(&self) -> Status {
        if self.hits == 0 {
            return Status::None;
        }
        let partial_jump = self
            .jumps
            .iter()
            .any(|jump| jump.true_hits == 0 || jump.false_hits == 0);
        let partial_switch = self
            .switches
            .iter()
            .any(|switch| switch.default_hits == 0 || switch.hits.contains(&0));
        match partial_jump || partial_switch {
            true => Status::Partial,
            false => Status::Full,
        }
    }

    /// The line's branches and those taken (`LineData.getBranchData`): two a jump, one a case,
    /// the default not counted.
    pub fn branches(&self) -> (u32, u32) {
        let jumps = self.jumps.iter().map(|jump| {
            (
                2,
                u32::from(jump.true_hits > 0) + u32::from(jump.false_hits > 0),
            )
        });
        let cases = self
            .switches
            .iter()
            .flat_map(|switch| switch.hits.iter().map(|hits| (1, u32::from(*hits > 0))));
        jumps
            .chain(cases)
            .fold((0, 0), |(total, covered), (all, taken)| {
                (total + all, covered + taken)
            })
    }

    fn merge(&mut self, other: &Line) {
        self.hits = self.hits.saturating_add(other.hits);
        for (jump, other) in self.jumps.iter_mut().zip(&other.jumps) {
            jump.true_hits = jump.true_hits.saturating_add(other.true_hits);
            jump.false_hits = jump.false_hits.saturating_add(other.false_hits);
        }
        for (switch, other) in self.switches.iter_mut().zip(&other.switches) {
            for (hits, other) in switch.hits.iter_mut().zip(&other.hits) {
                *hits = hits.saturating_add(*other);
            }
            switch.default_hits = switch.default_hits.saturating_add(other.default_hits);
        }
    }
}

/// A class's coverage: its lines, and its source file once found.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Class {
    /// Its binary name, `com.example.Calculator$Inner`.
    pub name: String,
    pub file: Option<PathBuf>,
    pub lines: Vec<Line>,
}

impl Class {
    /// The top-level class it belongs to (IntelliJ counts an inner class under it).
    pub fn top_level(&self) -> &str {
        self.name.split('$').next().unwrap_or(&self.name)
    }

    /// Its package, `com.example`; empty for the default package.
    pub fn package(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map_or("", |(package, _)| package)
    }

    /// Its name without the package, `Calculator$Inner`.
    pub fn simple_name(&self) -> &str {
        self.name
            .rsplit_once('.')
            .map_or(&self.name, |(_, name)| name)
    }

    pub fn counts(&self) -> Counts {
        let mut counts = Counts::default();
        let mut methods: BTreeMap<&str, bool> = BTreeMap::new();
        for line in &self.lines {
            let covered = line.status() != Status::None;
            counts.lines.add(covered);
            let (total, taken) = line.branches();
            counts.branches.total += total;
            counts.branches.covered += taken;
            *methods.entry(&line.method).or_default() |= covered;
        }
        for covered in methods.values() {
            counts.methods.add(*covered);
        }
        if !methods.is_empty() {
            counts.classes.add(counts.lines.covered > 0);
        }
        counts
    }
}

/// Covered of all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Ratio {
    pub covered: u32,
    pub total: u32,
}

impl Ratio {
    fn add(&mut self, covered: bool) {
        self.total += 1;
        self.covered += u32::from(covered);
    }

    fn plus(self, other: Ratio) -> Ratio {
        Ratio {
            covered: self.covered + other.covered,
            total: self.total + other.total,
        }
    }

    /// The percentage, truncated, 100 when there's nothing to cover (IntelliJ's).
    pub fn percent(&self) -> u32 {
        match self.total {
            0 => 100,
            total => (u64::from(self.covered) * 100 / u64::from(total)) as u32,
        }
    }

    /// IntelliJ's cell, `66% (2/3)` (`JavaCoverageAnnotator.getPercentage`).
    pub fn text(&self) -> String {
        format!("{}% ({}/{})", self.percent(), self.covered, self.total)
    }
}

/// IntelliJ's four counts of a class, a package or a folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub classes: Ratio,
    pub methods: Ratio,
    pub lines: Ratio,
    pub branches: Ratio,
}

impl Counts {
    pub fn plus(self, other: Counts) -> Counts {
        Counts {
            classes: self.classes.plus(other.classes),
            methods: self.methods.plus(other.methods),
            lines: self.lines.plus(other.lines),
            branches: self.branches.plus(other.branches),
        }
    }
}

/// A suite's coverage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Data {
    pub classes: Vec<Class>,
    /// Each test that ran with Track per test coverage, with the lines of each class it covered.
    pub tests: BTreeMap<String, BTreeMap<String, Vec<u32>>>,
}

impl Data {
    /// Adds another suite's coverage, as IntelliJ merges the active suites: a class's hits and
    /// branches summed, line by line.
    pub fn merge(&mut self, other: &Data) {
        for class in &other.classes {
            match self.classes.iter_mut().find(|own| own.name == class.name) {
                Some(own) => {
                    for line in &class.lines {
                        match own.lines.iter_mut().find(|own| own.line == line.line) {
                            Some(own) => own.merge(line),
                            None => own.lines.push(line.clone()),
                        }
                    }
                    own.lines.sort_by_key(|line| line.line);
                    if own.file.is_none() {
                        own.file = class.file.clone();
                    }
                }
                None => self.classes.push(class.clone()),
            }
        }
        self.classes.sort_by(|a, b| a.name.cmp(&b.name));
        for (test, classes) in &other.tests {
            let own = self.tests.entry(test.clone()).or_default();
            for (class, lines) in classes {
                let own = own.entry(class.clone()).or_default();
                own.extend(lines);
                own.sort_unstable();
                own.dedup();
            }
        }
    }

    /// The lines of the source file at `path`, from every class compiled from it, merged.
    pub fn lines_of(&self, path: &Path) -> BTreeMap<u32, Line> {
        let mut lines: BTreeMap<u32, Line> = BTreeMap::new();
        for class in self
            .classes
            .iter()
            .filter(|class| class.file.as_deref() == Some(path))
        {
            for line in &class.lines {
                match lines.get_mut(&line.line) {
                    Some(own) => own.merge(line),
                    None => {
                        lines.insert(line.line, line.clone());
                    }
                }
            }
        }
        lines
    }

    /// The counts of the classes compiled from the files under `folder` (or from that file).
    pub fn counts_under(&self, folder: &Path) -> Option<Counts> {
        let classes: Vec<&Class> = self
            .classes
            .iter()
            .filter(|class| {
                class
                    .file
                    .as_deref()
                    .is_some_and(|file| file.starts_with(folder))
            })
            .collect();
        (!classes.is_empty()).then(|| Self::counts_of(classes))
    }

    /// The counts of classes, an inner class counted with its top-level class as IntelliJ does.
    pub fn counts_of<'a>(classes: impl IntoIterator<Item = &'a Class>) -> Counts {
        let mut by_top: BTreeMap<&str, Counts> = BTreeMap::new();
        for class in classes {
            let counts = class.counts();
            let top = by_top.entry(class.top_level()).or_default();
            let classes = top.classes;
            *top = top.plus(Counts {
                classes: Ratio::default(),
                ..counts
            });
            // a top-level class is one class, covered if any of its classes is
            top.classes = Ratio {
                covered: u32::from(classes.covered > 0 || counts.classes.covered > 0),
                total: u32::from(classes.total > 0 || counts.classes.total > 0),
            };
        }
        by_top
            .into_values()
            .fold(Counts::default(), |all, counts| all.plus(counts))
    }

    /// The tests that covered `line` of the source file at `path`.
    pub fn tests_of_line(&self, path: &Path, line: u32) -> Vec<String> {
        let classes: Vec<&str> = self
            .classes
            .iter()
            .filter(|class| class.file.as_deref() == Some(path))
            .map(|class| class.name.as_str())
            .collect();
        self.tests
            .iter()
            .filter(|(_, covered)| {
                classes.iter().any(|class| {
                    covered
                        .get(*class)
                        .is_some_and(|lines| lines.contains(&line))
                })
            })
            .map(|(test, _)| test.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Class, Data, Jump, Line, Ratio, Status, Switch};

    fn line(line: u32, hits: u32, method: &str) -> Line {
        Line {
            line,
            hits,
            method: method.into(),
            ..Line::default()
        }
    }

    #[test]
    fn counts_as_intellij_does() {
        let mut branch = line(5, 3, "sign(I)");
        branch.jumps = vec![Jump {
            true_hits: 2,
            false_hits: 1,
        }];
        let mut switch = line(8, 1, "sign(I)");
        switch.switches = vec![Switch {
            keys: vec![-1, 0],
            hits: vec![0, 1],
            default_hits: 0,
        }];
        assert_eq!(branch.status(), Status::Full);
        assert_eq!(switch.status(), Status::Partial);
        assert_eq!(switch.branches(), (2, 1));
        let class = Class {
            name: "com.example.Calc".into(),
            file: None,
            lines: vec![
                line(3, 0, "<init>()V"),
                branch,
                switch,
                line(12, 0, "sign(I)"),
            ],
        };
        let inner = Class {
            name: "com.example.Calc$Inner".into(),
            file: None,
            lines: vec![line(20, 0, "run()V")],
        };
        let counts = Data::counts_of([&class, &inner]);
        assert_eq!(
            counts.classes,
            Ratio {
                covered: 1,
                total: 1
            }
        );
        assert_eq!(counts.methods.text(), "33% (1/3)");
        assert_eq!(counts.lines.text(), "40% (2/5)");
        assert_eq!(counts.branches.text(), "75% (3/4)");
        assert_eq!(Ratio::default().text(), "100% (0/0)");
    }

    #[test]
    fn merges_suites_line_by_line() {
        let class = |hits| Class {
            name: "A".into(),
            file: None,
            lines: vec![line(1, hits, "m()V")],
        };
        let mut data = Data {
            classes: vec![class(0)],
            ..Data::default()
        };
        data.merge(&Data {
            classes: vec![class(2)],
            ..Data::default()
        });
        assert_eq!(data.classes[0].lines[0].hits, 2);
    }
}
