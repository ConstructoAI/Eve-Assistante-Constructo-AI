package ca.constructoai.eve

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.sse.EventSource
import okhttp3.sse.EventSourceListener
import okhttp3.sse.EventSources
import org.json.JSONObject
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.URLEncoder
import java.net.UnknownHostException
import java.security.MessageDigest
import java.security.SecureRandom
import java.security.cert.CertificateException
import java.security.cert.X509Certificate
import java.util.Base64
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLHandshakeException
import javax.net.ssl.X509TrustManager

/** Le base64url sans remplissage, celui de l'ordinateur (`connexion::b64url`). */
fun base64url(o: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(o)

fun sha256(o: ByteArray): ByteArray = MessageDigest.getInstance("SHA-256").digest(o)

/**
 * N'accepte que le certificat de cette empreinte (le SHA-256 de son DER, en base64url) : celui de l'Eve jumelée, que
 * nomme son code carré. Aucune autorité ne le signe — c'est l'ordinateur qui l'a tiré — et rien de ce qu'il dit de
 * lui-même ne compte : seule son empreinte.
 */
class Epingle(private val empreinte: String) : X509TrustManager {
    override fun checkServerTrusted(chain: Array<X509Certificate>?, authType: String?) {
        val certificat = chain?.firstOrNull() ?: throw CertificateException("Aucun certificat.")
        if (base64url(sha256(certificat.encoded)) != empreinte) {
            throw CertificateException("Ce n'est pas l'Eve jumelée.")
        }
    }

    override fun checkClientTrusted(chain: Array<X509Certificate>?, authType: String?) =
        throw CertificateException("L'appli n'est pas un serveur.")

    override fun getAcceptedIssuers(): Array<X509Certificate> = arrayOf()
}

private val JSON = "application/json; charset=utf-8".toMediaType()
private val WAV = "audio/wav".toMediaType()
private val OCTETS = "application/octet-stream".toMediaType()

/** Un client HTTPS épinglé sur l'Eve de cette empreinte. Le nom de l'hôte ne vérifie rien de plus : l'ordinateur n'a pas
 * de nom de domaine, et son certificat est déjà le seul accepté. */
fun clientEpingle(empreinte: String, lectureEnSecondes: Long = 30): OkHttpClient {
    val epingle = Epingle(empreinte)
    val tls = SSLContext.getInstance("TLS").apply { init(null, arrayOf(epingle), SecureRandom()) }
    return OkHttpClient.Builder()
        .sslSocketFactory(tls.socketFactory, epingle)
        .hostnameVerifier { _, _ -> true }
        .connectTimeout(5, TimeUnit.SECONDS)
        .readTimeout(lectureEnSecondes, TimeUnit.SECONDS)
        .writeTimeout(60, TimeUnit.SECONDS)
        .build()
}

/** Ce qu'une panne de la liaison veut dire, pour la personne. */
fun explication(e: Throwable): String = when (e) {
    is SSLHandshakeException -> "Ce n'est pas l'Eve jumelée : son certificat a changé. Rejumelle ce téléphone."
    is ConnectException, is SocketTimeoutException, is UnknownHostException ->
        "L'ordinateur ne répond pas : est-il allumé, et sur le même Wi-Fi que ce téléphone ?"
    else -> "La liaison a échoué (${e.message ?: e.javaClass.simpleName})."
}

/**
 * Le jumelage : le téléphone se présente avec le code de l'offre et une clé à lui, tirée ici (256 bits), que le code
 * carré n'a jamais montrée — à chaque adresse de l'offre, jusqu'à ce que l'une réponde.
 */
suspend fun jumeler(offre: Offre, nomDuTelephone: String): Result<Jumelage> = withContext(Dispatchers.IO) {
    val cle = base64url(ByteArray(32).also { SecureRandom().nextBytes(it) })
    val corps = JSONObject().put("code", offre.code).put("cle", cle).put("nom", nomDuTelephone).toString()
    val client = clientEpingle(offre.empreinte)
    var raison = "L'ordinateur ne répond pas."
    for (adresse in offre.adresses) {
        val requete = Request.Builder().url("https://$adresse:${offre.port}/jumeler").post(corps.toRequestBody(JSON))
        try {
            client.newCall(requete.build()).execute().use { r ->
                val texte = r.body.string()
                if (r.code == 200) {
                    val o = JSONObject(texte)
                    // L'adresse qui a répondu d'abord : c'est par elle que le téléphone joindra l'ordinateur.
                    val adresses = listOf(adresse) + offre.adresses.filter { it != adresse }
                    val j = Jumelage(adresses, offre.port, offre.empreinte, o.optString("eve", offre.nom),
                        o.getString("id"), o.optString("qui"), cle)
                    return@withContext Result.success(j)
                }
                raison = texte.ifBlank { "L'ordinateur refuse ce jumelage (${r.code})." }
            }
        } catch (e: IOException) {
            raison = explication(e)
        }
    }
    Result.failure(IOException(raison))
}

/**
 * La liaison d'un téléphone jumelé avec l'Eve de l'ordinateur : chaque requête porte sa clé ; elle passe par la dernière
 * adresse qui a répondu, puis par les autres (le Wi-Fi de la maison, Tailscale).
 */
class Liaison(val jumelage: Jumelage) {
    private val client = clientEpingle(jumelage.empreinte)

    /** Le flux attend plus longtemps : l'ordinateur y bat toutes les quinze secondes. */
    private val clientDuFlux = client.newBuilder().readTimeout(45, TimeUnit.SECONDS).build()

    @Volatile
    private var adresse = jumelage.adresses.first()

    private fun requete(chemin: String) = Request.Builder()
        .url("https://$adresse:${jumelage.port}$chemin")
        .header("Authorization", "Bearer ${jumelage.cle}")

    /** La prochaine adresse, après une panne du flux. */
    fun autreAdresse() {
        val i = jumelage.adresses.indexOf(adresse)
        adresse = jumelage.adresses[(i + 1) % jumelage.adresses.size]
    }

    /** Le flux de l'Eve : son état, sa voix, sa coupure ([Modele]). */
    fun flux(ecoute: EventSourceListener): EventSource =
        EventSources.createFactory(clientDuFlux).newEventSource(requete("/flux").build(), ecoute)

    /** Un geste de la conversation (écrire, confirmer, annuler, arrêter, se taire…) ; son code HTTP (204 : reçu). */
    suspend fun geste(action: String, valeur: Any? = null): Int =
        poster("/action", JSONObject().put("action", action).put("valeur", valeur ?: JSONObject.NULL).toString()
            .toRequestBody(JSON)).first

    /** La voix de la personne, en WAV : l'oreille de l'ordinateur la transcrit. */
    suspend fun son(wav: ByteArray): Int = poster("/son", wav.toRequestBody(WAV)).first

    /** Une photo (ou un fichier) jointe à la prochaine demande ; le texte d'un refus. */
    suspend fun piece(nom: String, octets: ByteArray): Pair<Int, String> =
        poster("/piece?nom=" + URLEncoder.encode(nom, "UTF-8").replace("+", "%20"), octets.toRequestBody(OCTETS))

    /** Le portrait d'Eve, s'il y en a un sur l'ordinateur. */
    suspend fun portrait(): ByteArray? = withContext(Dispatchers.IO) {
        runCatching {
            client.newCall(requete("/portrait").build()).execute().use { r -> if (r.code == 200) r.body.bytes() else null }
        }.getOrNull()
    }

    private suspend fun poster(chemin: String, corps: RequestBody): Pair<Int, String> = withContext(Dispatchers.IO) {
        try {
            client.newCall(requete(chemin).post(corps).build()).execute().use { r -> r.code to r.body.string() }
        } catch (e: IOException) {
            0 to explication(e)
        }
    }
}
