plugins {
    id("com.android.application")
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

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}
