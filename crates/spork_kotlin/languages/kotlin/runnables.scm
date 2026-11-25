; A JVM entry point: a top-level `main` (its file's facade class), or a `main` in an object or
; a companion object (the object's class, the companion's enclosing class)
((source_file
  (function_declaration
    (simple_identifier) @run
    (#eq? @run "main")) @_)
  (#set! tag kotlin-main))

((object_declaration
  (type_identifier) @kotlin_main_class
  (class_body
    (function_declaration
      (simple_identifier) @run
      (#eq? @run "main")))) @_
  (#set! tag kotlin-main))

((class_declaration
  (type_identifier) @kotlin_main_class
  (class_body
    (companion_object
      (class_body
        (function_declaration
          (simple_identifier) @run
          (#eq? @run "main")))))) @_
  (#set! tag kotlin-main))

; A test function: a member function annotated `@Test` (JUnit 4 and 5, kotlin.test, TestNG)
((source_file
  (package_header
    (identifier) @kotlin_package_name)?
  (class_declaration
    (type_identifier) @kotlin_test_class
    (class_body
      (function_declaration
        (modifiers
          (annotation
            [
              (user_type
                (type_identifier) @_annotation)
              (constructor_invocation
                (user_type
                  (type_identifier) @_annotation))
            ]))
        (simple_identifier) @run @kotlin_test_method)))) @_
  (#eq? @_annotation "Test")
  (#set! tag kotlin-test-method))

; A test class: a class with a test function
((source_file
  (package_header
    (identifier) @kotlin_package_name)?
  (class_declaration
    (type_identifier) @run @kotlin_test_class
    (class_body
      (function_declaration
        (modifiers
          (annotation
            [
              (user_type
                (type_identifier) @_annotation)
              (constructor_invocation
                (user_type
                  (type_identifier) @_annotation))
            ])))))) @_
  (#eq? @_annotation "Test")
  (#set! tag kotlin-test-class))

; A task a Gradle Kotlin script declares (IntelliJ's `KotlinGradleTaskRunLineMarkerProvider`):
; `tasks.register("hello")` or `tasks.create("hello")`, `val hello by tasks.registering`
((call_expression
  (navigation_expression
    (simple_identifier) @_tasks
    (navigation_suffix
      (simple_identifier) @run))
  (call_suffix
    (value_arguments
      .
      (value_argument
        (string_literal
          (string_content) @kotlin_gradle_task))))) @_
  (#eq? @_tasks "tasks")
  (#any-of? @run "register" "create")
  (#set! tag gradle-task))

((property_declaration
  (variable_declaration
    (simple_identifier) @run @kotlin_gradle_task)
  (property_delegate
    [
      (navigation_expression
        (simple_identifier) @_tasks
        (navigation_suffix
          (simple_identifier) @_how))
      (call_expression
        (navigation_expression
          (simple_identifier) @_tasks
          (navigation_suffix
            (simple_identifier) @_how)))
    ])) @_
  (#eq? @_tasks "tasks")
  (#any-of? @_how "registering" "creating")
  (#set! tag gradle-task))
