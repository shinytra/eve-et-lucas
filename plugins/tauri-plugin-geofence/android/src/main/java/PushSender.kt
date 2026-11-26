package fr.eveetlucas.geofence

import android.util.Log
import java.net.URL
import javax.net.ssl.HttpsURLConnection

/**
 * Envoi d'une arrivée chiffrée aux adresses ntfy des autres appareils.
 *
 * Une requête `POST` par adresse, corps = message `EL1.…` tel quel, aucun
 * en-tête ntfy (ni titre ni étiquette : le serveur ne doit rien apprendre).
 * Pas de nouvel essai : la synchro iroh rattrape l'arrivée plus tard.
 */
object PushSender {
  /** Une adresse plus longue n'est pas une adresse ntfy : ignorée. */
  private const val MAX_TARGET_LEN = 500
  private const val MAX_TARGETS = 10
  private const val CONNECT_TIMEOUT_MS = 5_000
  private const val READ_TIMEOUT_MS = 5_000

  /** Au-delà, on rend la main au récepteur, réponses reçues ou non. */
  private const val TOTAL_TIMEOUT_MS = 10_000L

  /** Garde les adresses `https` d'au plus 500 caractères, sans doublon. */
  fun validTargets(targets: List<String>): List<String> =
    targets.asSequence()
      .map { it.trim() }
      .filter { it.length in 1..MAX_TARGET_LEN }
      .filter { target ->
        try {
          val url = URL(target)
          url.protocol == "https" && !url.host.isNullOrEmpty()
        } catch (e: Exception) {
          false
        }
      }
      .distinct()
      .take(MAX_TARGETS)
      .toList()

  /**
   * Chiffre et envoie [arrival] sur un fil d'arrière-plan, puis appelle
   * [done] (toujours, même en cas d'erreur, au plus tard après ~10 s).
   */
  fun send(config: PushConfig, arrival: ArrivalMessage, done: () -> Unit) {
    val key = PushCrypto.decodeKey(config.key)
    val targets = validTargets(config.targets)
    if (key == null || targets.isEmpty()) {
      done()
      return
    }
    val worker = Thread {
      try {
        val body = PushCrypto.encrypt(key, ArrivalPayload.encode(arrival).toByteArray(Charsets.UTF_8))
          .toByteArray(Charsets.UTF_8)
        val deadline = System.currentTimeMillis() + TOTAL_TIMEOUT_MS
        val posts = targets.map { target ->
          Thread { post(target, body) }.apply {
            isDaemon = true
            start()
          }
        }
        for (t in posts) {
          val left = deadline - System.currentTimeMillis()
          if (left > 0) t.join(left)
        }
      } catch (e: Throwable) {
        Log.w(TAG, "Envoi de l'arrivée par ntfy impossible", e)
      } finally {
        done()
      }
    }
    worker.isDaemon = true
    try {
      worker.start()
    } catch (e: Throwable) {
      Log.w(TAG, "Fil d'envoi ntfy impossible à lancer", e)
      done()
    }
  }

  private fun post(target: String, body: ByteArray) {
    // Pas l'adresse entière dans le journal : le nom du sujet ntfy sert de secret.
    val host = try {
      URL(target).host
    } catch (e: Exception) {
      "?"
    }
    var conn: HttpsURLConnection? = null
    try {
      conn = URL(target).openConnection() as HttpsURLConnection
      conn.requestMethod = "POST"
      conn.doOutput = true
      conn.useCaches = false
      // Une redirection pourrait mener vers http : refusée.
      conn.instanceFollowRedirects = false
      conn.connectTimeout = CONNECT_TIMEOUT_MS
      conn.readTimeout = READ_TIMEOUT_MS
      conn.setRequestProperty("Content-Type", "text/plain; charset=utf-8")
      conn.setFixedLengthStreamingMode(body.size)
      conn.outputStream.use { it.write(body) }
      val code = conn.responseCode
      if (code in 200..299) {
        Log.i(TAG, "Arrivée envoyée par ntfy ($host)")
      } else {
        Log.w(TAG, "Envoi ntfy refusé par $host : code $code")
      }
    } catch (e: Exception) {
      Log.w(TAG, "Envoi ntfy vers $host impossible : ${e.javaClass.simpleName}")
    } finally {
      conn?.disconnect()
    }
  }
}
