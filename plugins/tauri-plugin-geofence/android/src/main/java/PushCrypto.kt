package fr.eveetlucas.geofence

import android.util.Base64
import org.json.JSONObject
import java.security.GeneralSecurityException
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

/**
 * Chiffrement des messages envoyés par ntfy, identique octet pour octet à
 * celui du cœur Rust :
 *
 * - clé : 32 octets, transmise en base64 standard avec remplissage ;
 * - message : `"EL1." + base64(nonce (12 octets) || chiffré || tag (16 octets))`,
 *   AES-256-GCM, tag de 128 bits, sans données associées.
 *
 * Le serveur ntfy et l'appli ntfy ne voient que ce texte : sans la clé, ni
 * lecture ni falsification possibles (GCM authentifie le message).
 */
object PushCrypto {
  const val PREFIX = "EL1."
  const val KEY_LEN = 32
  private const val NONCE_LEN = 12
  private const val TAG_LEN = 16
  private const val TAG_BITS = TAG_LEN * 8

  /** Au-delà, ce n'est pas un message de l'appli (une arrivée fait ~250 caractères). */
  private const val MAX_MESSAGE_LEN = 4096

  private val random = SecureRandom()

  /** Clé en base64, ou `null` si elle est vide ou ne fait pas 32 octets. */
  fun decodeKey(keyB64: String): ByteArray? {
    val trimmed = keyB64.trim()
    if (trimmed.isEmpty()) return null
    return try {
      Base64.decode(trimmed, Base64.DEFAULT).takeIf { it.size == KEY_LEN }
    } catch (e: IllegalArgumentException) {
      null
    }
  }

  fun encrypt(key: ByteArray, plaintext: ByteArray): String {
    val nonce = ByteArray(NONCE_LEN)
    random.nextBytes(nonce)
    return encryptWithNonce(key, nonce, plaintext)
  }

  /** Séparé de [encrypt] pour les vecteurs de test : ne jamais réutiliser un nonce. */
  internal fun encryptWithNonce(key: ByteArray, nonce: ByteArray, plaintext: ByteArray): String {
    val cipher = Cipher.getInstance("AES/GCM/NoPadding")
    cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(key, "AES"), GCMParameterSpec(TAG_BITS, nonce))
    val sealed = cipher.doFinal(plaintext)
    val out = ByteArray(nonce.size + sealed.size)
    System.arraycopy(nonce, 0, out, 0, nonce.size)
    System.arraycopy(sealed, 0, out, nonce.size, sealed.size)
    return PREFIX + Base64.encodeToString(out, Base64.NO_WRAP)
  }

  /** Clair du message, ou `null` s'il est illisible, trafiqué ou chiffré avec une autre clé. */
  fun decrypt(key: ByteArray, message: String): ByteArray? {
    val text = message.trim()
    if (text.length > MAX_MESSAGE_LEN || !text.startsWith(PREFIX)) return null
    val raw = try {
      Base64.decode(text.substring(PREFIX.length), Base64.DEFAULT)
    } catch (e: IllegalArgumentException) {
      return null
    }
    if (raw.size < NONCE_LEN + TAG_LEN) return null
    return try {
      val cipher = Cipher.getInstance("AES/GCM/NoPadding")
      cipher.init(
        Cipher.DECRYPT_MODE,
        SecretKeySpec(key, "AES"),
        GCMParameterSpec(TAG_BITS, raw, 0, NONCE_LEN)
      )
      cipher.doFinal(raw, NONCE_LEN, raw.size - NONCE_LEN)
    } catch (e: GeneralSecurityException) {
      null
    } catch (e: IllegalArgumentException) {
      null
    }
  }
}

/** Une nouveauté annoncée par un autre appareil (textes déjà prêts à afficher). */
data class EventMessage(
  val kind: String,
  val id: String,
  val who: String,
  val device: String,
  val at: Long,
  val item: String,
  val title: String,
  val body: String
)

/**
 * Clair des nouveautés (préparé par `couple_core::notify`) :
 * `{"v":1,"kind":"note","id":…,"who":…,"device":…,"at":…,"item":…,"title":…,"body":…}`.
 * Type inconnu, autre version ou champ invalide : ignoré.
 */
object EventPayload {
  /** Les types de `couple_core::notify::KINDS`. */
  val KINDS = setOf("note", "tache", "tache-faite", "date", "anime", "fichier")
  const val SHOW_WITHIN_MS = 12 * 60 * 60 * 1000L
  const val CLOCK_SKEW_MS = 60_000L
  private const val MAX_TEXT = 120
  private const val MAX_WHO_LEN = 64
  private const val MAX_ID_LEN = 128

  private fun validId(id: String): Boolean =
    id.isNotEmpty() && id.length <= MAX_ID_LEN && id.all { it in '!'..'~' }

  /** Coupe à [MAX_TEXT] caractères, sur une seule ligne. */
  private fun short(text: String): String {
    val flat = text.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
      .filter { !it.isISOControl() }
    // Compté en caractères (points de code), comme en Rust : un émoji compte pour un.
    if (flat.codePointCount(0, flat.length) <= MAX_TEXT) return flat
    return flat.substring(0, flat.offsetByCodePoints(0, MAX_TEXT - 1)) + "…"
  }

  fun decode(text: String): EventMessage? {
    val json = try {
      JSONObject(text)
    } catch (e: Exception) {
      return null
    }
    val v = json.opt("v")
    if (!((v is Int && v == 1) || (v is Long && v == 1L))) return null
    val kind = json.opt("kind") as? String ?: return null
    if (kind !in KINDS) return null
    val id = json.opt("id") as? String ?: return null
    if (!validId(id)) return null
    val who = (json.opt("who") as? String)?.trim() ?: return null
    if (who.isEmpty() || who.codePointCount(0, who.length) > MAX_WHO_LEN) return null
    val device = json.opt("device") as? String ?: return null
    if (!validId(device)) return null
    val item = json.opt("item") as? String ?: ""
    if (item.isNotEmpty() && !validId(item)) return null
    val title = short(json.opt("title") as? String ?: return null)
    if (title.isEmpty()) return null
    val body = short(json.opt("body") as? String ?: "")
    val at = when (val raw = json.opt("at")) {
      is Int -> raw.toLong()
      is Long -> raw
      else -> return null
    }
    return EventMessage(kind, id, who, device, at, item, title, body)
  }
}

/** Une arrivée transmise par ntfy (celle d'un autre appareil, ou la nôtre à envoyer). */
data class ArrivalMessage(val id: String, val who: String, val device: String, val at: Long)

/**
 * Clair des messages : `{"v":1,"kind":"arrival","id":…,"who":…,"device":…,"at":…}`.
 * Champs inconnus ignorés ; autre version ou autre type : ignoré.
 */
object ArrivalPayload {
  private val UUID_RE =
    Regex("^[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}$")
  private const val MAX_WHO_LEN = 64
  private const val MAX_DEVICE_LEN = 128

  fun encode(arrival: ArrivalMessage): String {
    val json = JSONObject()
    json.put("v", 1)
    json.put("kind", "arrival")
    json.put("id", arrival.id)
    json.put("who", arrival.who)
    json.put("device", arrival.device)
    json.put("at", arrival.at)
    return json.toString()
  }

  /** Arrivée lue, ou `null` si le texte n'en est pas une (raison dans le journal de l'appelant). */
  fun decode(text: String): ArrivalMessage? {
    val json = try {
      JSONObject(text)
    } catch (e: Exception) {
      return null
    }
    val v = json.opt("v")
    if (!((v is Int && v == 1) || (v is Long && v == 1L))) return null
    if (json.opt("kind") != "arrival") return null
    val id = json.opt("id") as? String ?: return null
    if (!UUID_RE.matches(id)) return null
    val who = (json.opt("who") as? String)?.trim() ?: return null
    if (who.isEmpty() || who.length > MAX_WHO_LEN) return null
    val device = json.opt("device") as? String ?: return null
    if (device.isEmpty() || device.length > MAX_DEVICE_LEN) return null
    val at = when (val raw = json.opt("at")) {
      is Int -> raw.toLong()
      is Long -> raw
      else -> return null
    }
    return ArrivalMessage(id, who, device, at)
  }
}
