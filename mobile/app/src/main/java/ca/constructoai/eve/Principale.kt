package ca.constructoai.eve

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Color
import android.net.Uri
import android.os.Bundle
import android.provider.OpenableColumns
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.activity.viewModels
import androidx.core.content.ContextCompat
import ca.constructoai.eve.ui.Accueil
import ca.constructoai.eve.ui.Conversation
import ca.constructoai.eve.ui.JumelageEnCours
import ca.constructoai.eve.ui.ThemeEve
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions

/**
 * L'appli entière tient dans cette activité : Compose y dessine tout, comme egui dessine la fenêtre de l'ordinateur. Elle
 * reçoit aussi les liens `eve://jumeler…` (un code carré lu par l'appareil photo du téléphone, ou un essai), et demande
 * le micro au premier besoin.
 */
class Principale : ComponentActivity() {
    private val modele: Modele by viewModels()

    /** Le code carré, lu par ZXing : son texte va au jumelage. */
    private val scanner = registerForActivityResult(ScanContract()) { r -> r.contents?.let(modele::jumeler) }

    /** Une photo de la galerie (le sélecteur d'Android : aucune permission), jointe à la prochaine demande. */
    private val photo = registerForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        uri?.let(::joindre)
    }

    private val permissionDuMicro = registerForActivityResult(ActivityResultContracts.RequestPermission()) {}

    // Le nom du paramètre est celui d'Android : une redéfinition garde les noms de la fonction qu'elle remplace.
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Les barres du système sur le marine de la barre d'Eve : leurs icônes en blanc.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
        )
        lienRecu(intent)
        setContent {
            ThemeEve {
                when {
                    modele.jumelageEnCours -> JumelageEnCours()
                    modele.jumelage == null -> Accueil(scanner = ::scannerLeCode, message = modele.message)
                    else -> Conversation(modele, choisirUnePhoto = {
                        photo.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly))
                    })
                }
            }
        }
        demanderLeMicro()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        lienRecu(intent)
    }

    private fun lienRecu(intent: Intent?) {
        intent?.data?.toString()?.takeIf { it.startsWith("eve://") }?.let(modele::jumeler)
    }

    private fun scannerLeCode() {
        scanner.launch(
            ScanOptions()
                .setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                .setPrompt("Vise le code carré des réglages d'Eve, sur l'ordinateur")
                .setBeepEnabled(false)
                .setOrientationLocked(false)
        )
    }

    private fun demanderLeMicro() {
        val accorde = ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO)
        if (accorde != PackageManager.PERMISSION_GRANTED) permissionDuMicro.launch(Manifest.permission.RECORD_AUDIO)
    }

    /** Lit la photo choisie (son nom, ses octets) et la joint. */
    private fun joindre(uri: Uri) {
        val nom = contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        } ?: "photo.jpg"
        val octets = runCatching { contentResolver.openInputStream(uri)?.use { it.readBytes() } }.getOrNull() ?: return
        modele.joindre(nom, octets)
    }
}
