package com.example

/** Prints the greetings. */
fun main() {
    val names = listOf("world", "Kotlin")
    names.forEach { name ->
        val line = greet(name)
        println(line)
    }
}

fun greet(name: String): String {
    val greeting = Greeting(name)
    return greeting.text()
}
