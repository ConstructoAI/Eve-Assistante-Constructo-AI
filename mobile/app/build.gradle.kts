plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// La révision d'Eve : la `version` de la section [package] de Cargo.toml, sa seule source (« 26.0.0 » : la v26).
// L'appli porte le numéro du programme de l'ordinateur avec lequel elle se construit.
val revision: Int =
    rootDir.resolve("../Cargo.toml").readLines()
        .dropWhile { it.trim() != "[package]" }
        .drop(1)
        .takeWhile { !it.trimStart().startsWith("[") }
        .first { it.startsWith("version = ") }
        .substringAfter('"')
        .substringBefore('.')
        .toInt()

android {
    namespace = "ca.constructoai.eve"
    // Android 17 : Compose 1.12 le demande pour compiler. L'appli vise encore Android 16 (targetSdk) et part
    // d'Android 8 (minSdk).
    compileSdk = 37

    defaultConfig {
        applicationId = "ca.constructoai.eve"
        minSdk = 26
        targetSdk = 36
        versionCode = revision
        versionName = "$revision"
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    // Le Kotlin compile pour la même version de Java (AGP 9 la lui donne).
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }
}

dependencies {
    implementation(libs.androidx.core)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.okhttp)
    implementation(libs.okhttp.sse)
    implementation(libs.zxing)
    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
    testImplementation(libs.junit)
    testImplementation(libs.json)
}
