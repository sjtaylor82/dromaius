import java.util.Properties

plugins {
    id("com.android.application")
}

// Release signing key, kept outside version control in <repo>/signing/
// (see signing/README.txt). Without it, release builds fall back to the debug key.
val signingDir = rootProject.file("../signing")
val signingProps = Properties().apply {
    val file = File(signingDir, "keystore.properties")
    if (file.exists()) file.inputStream().use { load(it) }
}
if (signingProps.isEmpty && gradle.startParameter.taskNames.any { it.contains("release", ignoreCase = true) }) {
    throw GradleException("Release signing credentials are missing; refusing to create an unsigned or debug-signed bridge")
}

android {
    namespace = "com.dromaius.bridge"
    compileSdk = 36

    defaultConfig {
        applicationId = "com.dromaius.bridge"
        minSdk = 30
        targetSdk = 36
        versionCode = 1
        versionName = "0.1.0"
    }

    signingConfigs {
        if (!signingProps.isEmpty) {
            create("release") {
                storeFile = File(signingDir, signingProps.getProperty("storeFile"))
                storePassword = signingProps.getProperty("storePassword")
                keyAlias = signingProps.getProperty("keyAlias")
                keyPassword = signingProps.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            // Never let a distributable-looking release silently use Android's
            // well-known debug key. mac-test consumes the separately signed APK.
            signingConfig = signingConfigs.findByName("release")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
