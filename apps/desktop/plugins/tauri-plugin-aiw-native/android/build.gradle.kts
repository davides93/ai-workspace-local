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
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.15.0")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // Gemini Nano via ML Kit GenAI Prompt API (AICore). Versions verified on maven.google.com.
    implementation("com.google.mlkit:genai-prompt:1.0.0-beta5")
    // LiteRT-LM for .litertlm models (Gemma 3n / 4) on CPU, GPU and NPU.
    implementation("com.google.ai.edge.litertlm:litertlm-android:0.19.0")
    implementation(project(":tauri-android"))
}
