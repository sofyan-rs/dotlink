import java.util.Properties

plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.compose)
}

// Release signing from android/keystore.properties (not in git):
//   storeFile=/path/to/dotlink-release.jks
//   storePassword=…
//   keyAlias=dotlink
//   keyPassword=…
// Without it, release builds are signed with the debug key (installable, for personal use).
val keystoreProperties =
    Properties().apply {
        rootProject.file("keystore.properties").takeIf { it.exists() }?.inputStream()?.use(::load)
    }

// One version for the whole project: the desktop workspace's (desktop/Cargo.toml). The release
// workflow checks the tag against it.
val dotlinkVersion: String =
    Regex("""(?m)^version\s*=\s*"([^"]+)"""")
        .find(rootProject.file("../desktop/Cargo.toml").readText())
        ?.groupValues
        ?.get(1) ?: error("no version in desktop/Cargo.toml")

// Grows with every release, pre-releases included: 1.2.3-beta.4 is 1_02_03_04, 1.2.3 is
// 1_02_03_99 (newer than any of its pre-releases).
val dotlinkVersionCode: Int =
    run {
        val (core, pre) = dotlinkVersion.substringBefore('+').split('-', limit = 2).let { it[0] to it.getOrNull(1) }
        val (major, minor, patch) = core.split('.').map(String::toInt)
        val preNumber = pre?.let { Regex("""\d+""").findAll(it).lastOrNull()?.value?.toInt() ?: 0 } ?: 99
        require(minor < 100 && patch < 100 && preNumber < 100) { "version $dotlinkVersion doesn't fit the version code" }
        major * 1_000_000 + minor * 10_000 + patch * 100 + preNumber
    }

android {
    namespace = "com.dotlink"
    compileSdk = 37

    defaultConfig {
        applicationId = "com.dotlink.android"
        minSdk = 29
        targetSdk = 36
        versionCode = dotlinkVersionCode
        versionName = dotlinkVersion
        // Phones only: ML Kit's barcode library is ~6 MB per ABI, and x86 builds are for emulators.
        ndk { abiFilters += listOf("arm64-v8a", "armeabi-v7a") }
    }

    signingConfigs {
        if (keystoreProperties.isNotEmpty()) {
            create("release") {
                storeFile = file(keystoreProperties.getProperty("storeFile"))
                storePassword = keystoreProperties.getProperty("storePassword")
                keyAlias = keystoreProperties.getProperty("keyAlias")
                keyPassword = keystoreProperties.getProperty("keyPassword")
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.findByName("release") ?: signingConfigs.getByName("debug")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures {
        compose = true
    }
}

dependencies {
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.service)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.okhttp)
    implementation(libs.camera.core)
    implementation(libs.camera.camera2)
    implementation(libs.camera.lifecycle)
    implementation(libs.camera.view)
    implementation(libs.mlkit.barcode)

    implementation(platform(libs.compose.bom))
    implementation(libs.compose.ui)
    implementation(libs.compose.foundation)
    implementation(libs.compose.material3)
    implementation(libs.compose.ui.tooling.preview)
    debugImplementation(libs.compose.ui.tooling)
}
