; The names whose values the debugger shows after a line (IntelliJ's inline values): parameters,
; locals and the names a line uses.

(formal_parameter
  name: (identifier) @debug-variable)

(catch_formal_parameter
  name: (identifier) @debug-variable)

(local_variable_declaration
  declarator: (variable_declarator
    name: (identifier) @debug-variable))

(enhanced_for_statement
  name: (identifier) @debug-variable)

(assignment_expression
  left: (identifier) @debug-variable)

(update_expression
  (identifier) @debug-variable)

(binary_expression
  (identifier) @debug-variable
  (#not-match? @debug-variable "^[A-Z]"))

(argument_list
  (identifier) @debug-variable
  (#not-match? @debug-variable "^[A-Z]"))

(return_statement
  (identifier) @debug-variable)

(method_invocation
  object: (identifier) @debug-variable
  (#not-match? @debug-variable "^[A-Z]"))

(block) @debug-scope

(method_declaration) @debug-scope

(constructor_declaration) @debug-scope

(lambda_expression) @debug-scope
