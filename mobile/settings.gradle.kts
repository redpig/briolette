pluginManagement {
    repositories {
        google()
        gradlePluginPortal()
        mavenCentral()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "BrioletteWallet"
include(":shared")
include(":androidApp")
include(":pos:shared")
include(":pos:androidApp")
