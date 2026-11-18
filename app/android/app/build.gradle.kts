plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

android {
    namespace = "io.github.poldion.tephra"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        applicationId = "io.github.poldion.tephra"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    // Signiert wird mit dem eigenen Schlüssel, wenn er angegeben ist (TEPHRA_KEYSTORE,
    // TEPHRA_KEYSTORE_PASSWORD, TEPHRA_KEY_ALIAS, TEPHRA_KEY_PASSWORD). Ohne ihn nimmt der Build den
    // Debug-Schlüssel, damit sich die App zum Ausprobieren bauen lässt; so entsteht nie ein Release.
    val keystore = System.getenv("TEPHRA_KEYSTORE")
    val releaseSigning = if (keystore != null) signingConfigs.create("release") {
        storeFile = file(keystore)
        storePassword = System.getenv("TEPHRA_KEYSTORE_PASSWORD")
        keyAlias = System.getenv("TEPHRA_KEY_ALIAS")
        keyPassword = System.getenv("TEPHRA_KEY_PASSWORD")
    } else signingConfigs.getByName("debug")

    buildTypes {
        release {
            signingConfig = releaseSigning
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
