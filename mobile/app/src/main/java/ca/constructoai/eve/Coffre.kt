package ca.constructoai.eve

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Le coffre du jumelage : une clé AES de l'AndroidKeyStore — elle ne sort jamais du téléphone, et une copie de ses
 * fichiers ne la porte pas — chiffre ce que l'appli garde de son jumelage dans ses préférences (la clé du téléphone,
 * surtout). L'appli ne part pas dans les sauvegardes (`allowBackup`, `dataExtractionRules`).
 */
class Coffre(contexte: Context) {
    private val preferences = contexte.getSharedPreferences("eve", Context.MODE_PRIVATE)

    fun lire(): Jumelage? =
        preferences.getString(JUMELAGE, null)?.let(::dechiffrer)?.let(Jumelage::deJson)

    fun ecrire(jumelage: Jumelage) {
        preferences.edit().putString(JUMELAGE, chiffrer(jumelage.enJson())).apply()
    }

    fun oublier() {
        preferences.edit().remove(JUMELAGE).apply()
    }

    private fun cle(): SecretKey {
        val magasin = KeyStore.getInstance(MAGASIN).apply { load(null) }
        (magasin.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generateur = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, MAGASIN)
        generateur.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build()
        )
        return generateur.generateKey()
    }

    private fun chiffrer(clair: String): String {
        val c = Cipher.getInstance(CHIFFRE)
        c.init(Cipher.ENCRYPT_MODE, cle())
        return Base64.encodeToString(c.iv + c.doFinal(clair.toByteArray()), Base64.NO_WRAP)
    }

    /** Rien si le coffre ne s'ouvre plus (l'appli réinstallée, une clé perdue) : le téléphone se rejumelle. */
    private fun dechiffrer(s: String): String? = runCatching {
        val o = Base64.decode(s, Base64.NO_WRAP)
        val c = Cipher.getInstance(CHIFFRE)
        c.init(Cipher.DECRYPT_MODE, cle(), GCMParameterSpec(128, o, 0, IV))
        String(c.doFinal(o, IV, o.size - IV))
    }.getOrNull()

    private companion object {
        const val JUMELAGE = "jumelage"
        const val MAGASIN = "AndroidKeyStore"
        const val ALIAS = "eve-jumelage"
        const val CHIFFRE = "AES/GCM/NoPadding"
        const val IV = 12
    }
}
