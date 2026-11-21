plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}
val root = rootProject.projectDir.parentFile
val rustVersion = Regex("""(?m)^version = "([^"]+)"""").find(root.resolve("Cargo.toml").readText())!!.groupValues[1]
val qaBuild = providers.gradleProperty("schoolDayQa").isPresent
val nativeLibs = layout.buildDirectory.dir("generated/rust")
android {
    namespace = "io.github.sqeezy53.schoolday"
    compileSdk = 36
    ndkVersion = "27.2.12479018"
    defaultConfig {
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        applicationId = "io.github.sqeezy53.schoolday"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
        minSdk = 26
        targetSdk = 36
        versionCode = rustVersion.split(".").map(String::toInt).let { it[0]*1000000 + it[1]*1000 + it[2] }
        versionName = rustVersion
    }
    buildTypes.getByName("debug") {
        if (qaBuild) applicationIdSuffix = ".qa"
    }
    defaultConfig.manifestPlaceholders["schoolDayLabel"] = if (qaBuild) "Школьный день QA" else "Школьный день"
    buildFeatures { compose = true; buildConfig = true }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    sourceSets["main"].jniLibs.srcDir(nativeLibs)
}
val rustBuild = tasks.register<Exec>("buildRust") {
    inputs.files(root.resolve("Cargo.lock"),root.resolve("Cargo.toml"), root.resolve("crates"),rootProject.file("build-rust.py"))
    outputs.dir(nativeLibs)
    commandLine("python3", rootProject.file("build-rust.py"), android.sdkDirectory.resolve("ndk/${android.ndkVersion}"),nativeLibs.get().asFile)
}
tasks.named("preBuild").configure { dependsOn(rustBuild) }
dependencies {
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation(platform("androidx.compose:compose-bom:2025.09.01"))
    implementation("androidx.compose.material3:material3")
    implementation("com.google.zxing:core:3.5.4")
}

// Robolectric calls the actual JNI entry point using a library built for the host.
val hostRust = tasks.register<Exec>("buildHostRust") {
    workingDir(root)
    commandLine("cargo", "build", "--locked", "-p", "school-day-android", "--target-dir", layout.buildDirectory.dir("host-rust").get().asFile)
}
android.testOptions {
    unitTests.isIncludeAndroidResources = true
    unitTests.all {
        it.dependsOn(hostRust)
        // Each sandbox loads the same host JNI library in its own classloader.
        it.forkEvery = 1
        it.systemProperty("java.library.path", layout.buildDirectory.dir("host-rust/debug").get().asFile.absolutePath)
        it.systemProperty("robolectric.dependency.repo.url", "https://repo.maven.apache.org/maven2")
    }
}
dependencies {
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.robolectric:robolectric:4.14.1")
}

dependencies {
    androidTestImplementation(platform("androidx.compose:compose-bom:2025.09.01"))
    androidTestImplementation("androidx.compose.ui:ui-test-junit4")
    androidTestImplementation("androidx.test:runner:1.6.2")
}
