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
