// L'appli Android d'Eve : une télécommande de l'Eve de l'ordinateur — le téléphone en est une fenêtre, un micro et un
// haut-parleur de plus ; le cerveau (Claude Code, les fichiers, l'ERP, la voix) reste sur l'ordinateur. Elle se
// construit par Construire.bat, à côté de ce fichier.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "Eve"
include(":app")
