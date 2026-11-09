plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "dev.aiworkspace.nativellm"
    compileSdk = 36

    defaultConfig {
        minSdk = 26
        consumerProguardFiles("consumer-rules.pro")
    }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

kotlin {
    compilerOptions {
        jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17)
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    // Proxy override for the agent's in-app browser (policy proxy on loopback).
    implementation("androidx.webkit:webkit:1.12.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.11.0")
    // Gemini Nano via ML Kit GenAI Prompt API (AICore). Versions verified on maven.google.com;
    // both libraries are built with Kotlin 2.4, so the app project must use Kotlin Gradle plugin 2.4+.
    implementation("com.google.mlkit:genai-prompt:1.0.0-beta5")
    // LiteRT-LM for .litertlm models (Gemma 3n / 4) on CPU, GPU and NPU.
    implementation("com.google.ai.edge.litertlm:litertlm-android:0.19.0")
    implementation(project(":tauri-android"))
}
