plugins {
    java
}

repositories {
    mavenCentral()
}

tasks.register("hello") {
    doLast {
        println("Hello from Gradle")
    }
}
