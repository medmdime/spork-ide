package com.example

import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking

/** Doubles a number after a pause. */
suspend fun work(n: Int): Int {
    delay(10)
    return n * 2
}

fun main() = runBlocking {
    val doubled = async { work(21) }
    println(doubled.await())
}
