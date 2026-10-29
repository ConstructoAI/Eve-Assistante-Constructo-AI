// Les greffons de la construction, déclarés une fois ici et appliqués par le module de l'appli (app\build.gradle.kts) ;
// leurs versions sont dans gradle\libs.versions.toml. Celui de Kotlin ne s'applique nulle part : le greffon d'Android
// compile le Kotlin lui-même (AGP 9), et le déclarer ici lui donne la version de Kotlin qu'attend celui de Compose.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.compose) apply false
}
