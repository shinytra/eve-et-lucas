import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "fr.eveetlucas.geofence"
    compileSdk = 36

    defaultConfig {
        minSdk = 24

        consumerProguardFiles("consumer-rules.pro")
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }
}

kotlin {
    compilerOptions {
        jvmTarget = JvmTarget.JVM_1_8
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.9.0")
    implementation("androidx.appcompat:appcompat:1.6.0")
    // Géofencing et position (services Google Play).
    implementation("com.google.android.gms:play-services-location:21.3.0")
    // Réception par ntfy (UnifiedPush). Compilée avec Kotlin 2.2.10, comme le
    // projet Android généré par la CLI Tauri 2.12 : à garder alignés.
    implementation("org.unifiedpush.android:connector:3.3.6")
    implementation(project(":tauri-android"))
}
