package ca.constructoai.eve.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

/** Les couleurs de Material dans celles de l'ERP : ses boutons, ses champs et ses cartes prennent le bleu de Constructo
 * AI et le blanc de ses cartes, sur le fond pâle de l'ERP. */
private val couleurs = lightColorScheme(
    primary = Erp.Primaire,
    onPrimary = Color.White,
    primaryContainer = Erp.Fond2,
    onPrimaryContainer = Erp.Marine,
    secondary = Erp.Azur,
    onSecondary = Color.White,
    background = Color(0xFFEFF5FF),
    onBackground = Erp.Texte,
    surface = Color.White,
    onSurface = Erp.Texte,
    onSurfaceVariant = Erp.Texte2,
    outline = Erp.FiletDeCarte,
    error = Erp.DangerEncre,
)

@Composable
fun ThemeEve(contenu: @Composable () -> Unit) {
    MaterialTheme(colorScheme = couleurs, content = contenu)
}
