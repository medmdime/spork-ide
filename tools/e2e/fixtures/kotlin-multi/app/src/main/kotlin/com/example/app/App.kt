package com.example.app

/** Says what it runs on: its classpath's last entry. */
fun main() {
    println("Hello from app")
    println(System.getProperty("java.class.path").substringAfterLast('/'))
}
