package com.example

/** A greeting for someone. */
class Greeting(private val name: String) {
    fun text(): String {
        return "Hello, $name!"
    }
}
