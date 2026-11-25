//! A Maven project's model as its `pom.xml` writes it, for the Maven tool window: the project's
//! coordinates, its modules, profiles, declared plugins and dependencies, with `${…}` properties
//! filled in from the POM's own.
// shortcut: no parent POM, BOM or super POM is read (Maven's effective model isn't computed);
// read JDT LS's m2e model once it exposes one, or run `help:effective-pom`, when a project needs it

use anyhow::{Context as _, Result};
use collections::HashMap;
use quick_xml::Reader;
use quick_xml::events::Event;

/// An XML element: its name, its text and its child elements.
#[derive(Debug, Default)]
pub struct Element {
    pub name: String,
    pub text: String,
    pub children: Vec<Element>,
}

impl Element {
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|child| child.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |child| child.name == name)
    }

    /// The text of the child of that name, trimmed; `None` when it is missing or empty.
    pub fn text_of(&self, name: &str) -> Option<String> {
        let text = self.child(name)?.text.trim();
        (!text.is_empty()).then(|| text.to_owned())
    }

    /// The element at a path of child names.
    pub fn at(&self, path: &[&str]) -> Option<&Element> {
        path.iter()
            .try_fold(self, |element, name| element.child(name))
    }
}

/// Reads an XML document into its root element; namespaces prefixes are left out of names.
pub fn parse_xml(text: &str) -> Result<Element> {
    let mut reader = Reader::from_str(text);
    let mut stack = vec![Element::default()];
    loop {
        match reader.read_event().context("reading XML")? {
            Event::Start(start) => stack.push(Element {
                name: String::from_utf8_lossy(start.local_name().as_ref()).into_owned(),
                ..Element::default()
            }),
            Event::Empty(start) => {
                let element = Element {
                    name: String::from_utf8_lossy(start.local_name().as_ref()).into_owned(),
                    ..Element::default()
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(element);
                }
            }
            Event::End(_) => {
                let element = stack.pop().context("an end tag without its start")?;
                stack
                    .last_mut()
                    .context("an end tag without its start")?
                    .children
                    .push(element);
            }
            Event::Text(text) => {
                if let Some(element) = stack.last_mut() {
                    element
                        .text
                        .push_str(&text.decode().context("decoding XML text")?);
                }
            }
            Event::CData(data) => {
                if let Some(element) = stack.last_mut() {
                    element
                        .text
                        .push_str(&data.decode().context("decoding XML text")?);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    let document = stack.pop().context("an XML document without its root")?;
    document
        .children
        .into_iter()
        .next()
        .context("an XML document without its root")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dependency {
    pub group_id: String,
    pub artifact_id: String,
    pub version: Option<String>,
    /// `compile` when the POM names none.
    pub scope: String,
}

impl Dependency {
    /// IntelliJ's text for it: `junit:junit:4.13.2`.
    pub fn coordinates(&self) -> String {
        match &self.version {
            Some(version) => format!("{}:{}:{version}", self.group_id, self.artifact_id),
            None => format!("{}:{}", self.group_id, self.artifact_id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plugin {
    pub group_id: String,
    pub artifact_id: String,
    pub version: Option<String>,
    /// The goals its executions name.
    pub goals: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pom {
    pub group_id: Option<String>,
    pub artifact_id: String,
    pub version: Option<String>,
    pub name: Option<String>,
    pub packaging: String,
    pub modules: Vec<String>,
    pub profiles: Vec<String>,
    pub dependencies: Vec<Dependency>,
    pub plugins: Vec<Plugin>,
}

impl Pom {
    pub fn parse(text: &str) -> Result<Self> {
        let project = parse_xml(text)?;
        anyhow::ensure!(project.name == "project", "not a Maven POM");
        let parent = project.child("parent");
        let group_id = project
            .text_of("groupId")
            .or_else(|| parent.and_then(|parent| parent.text_of("groupId")));
        let version = project
            .text_of("version")
            .or_else(|| parent.and_then(|parent| parent.text_of("version")));
        let artifact_id = project.text_of("artifactId").context("no artifactId")?;
        let mut properties: HashMap<String, String> = project
            .child("properties")
            .map(|properties| {
                properties
                    .children
                    .iter()
                    .map(|property| (property.name.clone(), property.text.trim().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        for (key, value) in [
            ("project.groupId", &group_id),
            ("project.artifactId", &Some(artifact_id.clone())),
            ("project.version", &version),
        ] {
            if let Some(value) = value {
                properties.insert(key.to_owned(), value.clone());
            }
        }
        let fill = |text: Option<String>| text.map(|text| interpolate(&text, &properties));
        let dependencies = project
            .child("dependencies")
            .map(|dependencies| {
                dependencies
                    .all("dependency")
                    .filter_map(|dependency| {
                        Some(Dependency {
                            group_id: fill(dependency.text_of("groupId"))?,
                            artifact_id: fill(dependency.text_of("artifactId"))?,
                            version: fill(dependency.text_of("version")),
                            scope: fill(dependency.text_of("scope"))
                                .unwrap_or_else(|| "compile".to_owned()),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let plugins = project
            .at(&["build", "plugins"])
            .map(|plugins| {
                plugins
                    .all("plugin")
                    .filter_map(|plugin| {
                        let goals = plugin
                            .child("executions")
                            .into_iter()
                            .flat_map(|executions| executions.all("execution"))
                            .filter_map(|execution| execution.child("goals"))
                            .flat_map(|goals| goals.all("goal"))
                            .map(|goal| interpolate(goal.text.trim(), &properties))
                            .collect();
                        Some(Plugin {
                            group_id: fill(plugin.text_of("groupId"))
                                .unwrap_or_else(|| "org.apache.maven.plugins".to_owned()),
                            artifact_id: fill(plugin.text_of("artifactId"))?,
                            version: fill(plugin.text_of("version")),
                            goals,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Self {
            name: fill(project.text_of("name")),
            packaging: fill(project.text_of("packaging")).unwrap_or_else(|| "jar".to_owned()),
            modules: project
                .child("modules")
                .map(|modules| {
                    modules
                        .all("module")
                        .map(|module| module.text.trim().to_owned())
                        .collect()
                })
                .unwrap_or_default(),
            profiles: project
                .child("profiles")
                .map(|profiles| {
                    profiles
                        .all("profile")
                        .filter_map(|profile| profile.text_of("id"))
                        .collect()
                })
                .unwrap_or_default(),
            group_id: fill(group_id),
            artifact_id,
            version: fill(version),
            dependencies,
            plugins,
        })
    }
}

/// Fills in `${name}` from the properties, leaving an unknown one as written.
fn interpolate(text: &str, properties: &HashMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        match after.find('}') {
            Some(end) => {
                let name = &after[..end];
                match properties.get(name) {
                    Some(value) => out.push_str(value),
                    None => out.push_str(&rest[start..start + 2 + end + 1]),
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(&rest[start..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::Pom;

    #[test]
    fn reads_coordinates_dependencies_and_plugins_with_their_properties() {
        let pom = Pom::parse(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <groupId>com.example</groupId>
  <artifactId>fixture</artifactId>
  <version>1.0-SNAPSHOT</version>
  <properties><junit.version>4.13.2</junit.version></properties>
  <dependencies>
    <dependency>
      <groupId>junit</groupId><artifactId>junit</artifactId>
      <version>${junit.version}</version><scope>test</scope>
    </dependency>
    <dependency><groupId>com.example</groupId><artifactId>lib</artifactId><version>${project.version}</version></dependency>
  </dependencies>
  <build><plugins><plugin>
    <artifactId>maven-jar-plugin</artifactId>
    <executions><execution><goals><goal>test-jar</goal></goals></execution></executions>
  </plugin></plugins></build>
  <profiles><profile><id>release</id></profile></profiles>
</project>"#,
        )
        .expect("a POM");
        assert_eq!(pom.artifact_id, "fixture");
        assert_eq!(pom.packaging, "jar");
        assert_eq!(pom.dependencies[0].coordinates(), "junit:junit:4.13.2");
        assert_eq!(pom.dependencies[0].scope, "test");
        assert_eq!(
            pom.dependencies[1].coordinates(),
            "com.example:lib:1.0-SNAPSHOT"
        );
        assert_eq!(pom.dependencies[1].scope, "compile");
        assert_eq!(pom.plugins[0].group_id, "org.apache.maven.plugins");
        assert_eq!(pom.plugins[0].goals, vec!["test-jar".to_owned()]);
        assert_eq!(pom.profiles, vec!["release".to_owned()]);
    }
}
