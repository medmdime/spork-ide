//! A Gradle project's model for the Gradle tool window, as Gradle reports it: its tasks by group
//! (`gradlew tasks --all`) and the dependencies of each configuration (`gradlew dependencies`),
//! both read from one run of the project's wrapper.

/// A task group and its tasks, as the Gradle tool window lists them: the group's name lowercase
/// (`build`, `help`), tasks of no group under `other`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskGroup {
    pub name: String,
    pub tasks: Vec<Task>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    pub name: String,
    pub description: String,
}

/// A configuration (`compileClasspath`) and its dependencies, each with what it brings in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Configuration {
    pub name: String,
    pub dependencies: Vec<DependencyNode>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DependencyNode {
    /// As Gradle writes it: `com.google.guava:guava:33.0.0-jre`.
    pub coordinates: String,
    pub children: Vec<DependencyNode>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GradleModel {
    pub tasks: Vec<TaskGroup>,
    pub configurations: Vec<Configuration>,
}

impl GradleModel {
    /// Reads the output of `gradlew tasks --all dependencies --console=plain`.
    pub fn parse(output: &str) -> Self {
        let mut model = Self::default();
        let mut section = "";
        let mut lines = output.lines().peekable();
        let mut group: Option<TaskGroup> = None;
        let mut configuration: Option<(Configuration, Vec<(usize, DependencyNode)>)> = None;
        let mut done_roots: Vec<DependencyNode> = Vec::new();
        while let Some(line) = lines.next() {
            if let Some(task) = line.strip_prefix("> Task :") {
                section = match task.trim() {
                    "tasks" => "tasks",
                    "dependencies" => "dependencies",
                    _ => "",
                };
                continue;
            }
            match section {
                "tasks" => {
                    // a group's title is underlined with dashes
                    if lines
                        .peek()
                        .is_some_and(|next| !next.is_empty() && next.chars().all(|c| c == '-'))
                        && let Some(title) = line.strip_suffix(" tasks")
                    {
                        model.tasks.extend(group.take());
                        group = Some(TaskGroup {
                            name: title.to_lowercase(),
                            tasks: Vec::new(),
                        });
                        lines.next();
                        continue;
                    }
                    if line.trim().is_empty() || line.starts_with("Tasks runnable") {
                        continue;
                    }
                    if line.starts_with("To see") || line.starts_with("BUILD ") {
                        model.tasks.extend(group.take());
                        continue;
                    }
                    if let Some(group) = &mut group {
                        let (name, description) = match line.split_once(" - ") {
                            Some((name, description)) => (name, description),
                            None => (line, ""),
                        };
                        if !name.contains(' ') {
                            group.tasks.push(Task {
                                name: name.to_owned(),
                                description: description.to_owned(),
                            });
                        }
                    }
                }
                "dependencies" => {
                    let tree = line.trim_start_matches(['|', ' ']);
                    let marker = tree.strip_prefix("+--- ").or(tree.strip_prefix("\\--- "));
                    match marker {
                        Some(coordinates) => {
                            if let Some((_, stack)) = &mut configuration {
                                let depth = (line.len() - tree.len()) / 5;
                                let node = DependencyNode {
                                    coordinates: coordinates
                                        .trim_end_matches(" (*)")
                                        .trim_end_matches(" (n)")
                                        .to_owned(),
                                    children: Vec::new(),
                                };
                                close_to(stack, &mut done_roots, depth);
                                stack.push((depth, node));
                            }
                        }
                        None => {
                            if let Some((mut done, mut stack)) = configuration.take() {
                                close_to(&mut stack, &mut done_roots, 0);
                                done.dependencies = std::mem::take(&mut done_roots);
                                model.configurations.push(done);
                            }
                            // Gradle's legend after the configurations: `(n) - A dependency…`
                            if let Some((name, _)) = line.split_once(" - ")
                                && !name.contains(' ')
                                && !name.is_empty()
                                && !name.starts_with('(')
                            {
                                configuration = Some((
                                    Configuration {
                                        name: name.to_owned(),
                                        dependencies: Vec::new(),
                                    },
                                    Vec::new(),
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        model.tasks.extend(group.take());
        if let Some((mut done, mut stack)) = configuration.take() {
            close_to(&mut stack, &mut done_roots, 0);
            done.dependencies = done_roots;
            model.configurations.push(done);
        }
        model.tasks.sort_by(|a, b| a.name.cmp(&b.name));
        model
    }
}

/// Ends the nodes at `depth` or deeper, each into its parent or, at the top, into `roots`.
fn close_to(
    stack: &mut Vec<(usize, DependencyNode)>,
    roots: &mut Vec<DependencyNode>,
    depth: usize,
) {
    while stack.last().is_some_and(|(top, _)| *top >= depth) {
        let Some((_, node)) = stack.pop() else {
            break;
        };
        match stack.last_mut() {
            Some((_, parent)) => parent.children.push(node),
            None => roots.push(node),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GradleModel;

    const OUTPUT: &str = "\
> Task :tasks

------------------------------------------------------------
Tasks runnable from root project 'fixture'
------------------------------------------------------------

Application tasks
-----------------
run - Runs this project as a JVM application

Build tasks
-----------
assemble - Assembles the outputs of this project.
build - Assembles and tests this project.

Other tasks
-----------
compileJava - Compiles main Java source.

To see all tasks and more detail, run gradlew tasks --all

> Task :dependencies

------------------------------------------------------------
Root project 'fixture'
------------------------------------------------------------

compileClasspath - Compile classpath for source set 'main'.
\\--- com.google.guava:guava:33.0.0-jre
     +--- com.google.guava:failureaccess:1.0.2
     \\--- com.google.j2objc:j2objc-annotations:2.8

testCompileClasspath - Compile classpath for source set 'test'.
+--- com.google.guava:guava:33.0.0-jre (*)
\\--- junit:junit:4.13.2
     \\--- org.hamcrest:hamcrest-core:1.3

(*) - Indicates repeated occurrences of a transitive dependency subtree.
(n) - A dependency or dependency configuration that cannot be resolved.

BUILD SUCCESSFUL in 1s
";

    #[test]
    fn reads_task_groups_and_dependency_trees() {
        let model = GradleModel::parse(OUTPUT);
        let groups: Vec<_> = model.tasks.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(groups, ["application", "build", "other"]);
        assert_eq!(model.tasks[1].tasks[1].name, "build");
        assert_eq!(
            model.tasks[1].tasks[1].description,
            "Assembles and tests this project."
        );
        let configurations: Vec<_> = model
            .configurations
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(configurations, ["compileClasspath", "testCompileClasspath"]);
        let guava = &model.configurations[0].dependencies[0];
        assert_eq!(guava.coordinates, "com.google.guava:guava:33.0.0-jre");
        assert_eq!(guava.children.len(), 2);
        let test = &model.configurations[1].dependencies;
        assert_eq!(test.len(), 2);
        assert_eq!(test[0].coordinates, "com.google.guava:guava:33.0.0-jre");
        assert_eq!(
            test[1].children[0].coordinates,
            "org.hamcrest:hamcrest-core:1.3"
        );
    }
}
