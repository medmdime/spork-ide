; IntelliJ's HTTP Request colours: the method as a keyword, the URL, header names and values,
; variables, comments and the request separator.
(comment) @comment
(request_separator) @comment

(method) @keyword
(target_url) @link_uri
(http_version) @constant

(header
  name: (header_entity) @property
  value: (value) @string)

(variable) @variable.special

(variable_declaration
  name: (identifier) @variable
  value: (value) @string)
