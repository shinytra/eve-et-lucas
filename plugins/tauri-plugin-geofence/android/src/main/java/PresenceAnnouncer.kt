package fr.eveetlucas.geofence

import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Ce que fait ce téléphone après une entrée ou une sortie de lieu : prévenir
 * l'appli si elle tourne, envoyer le message chiffré aux autres appareils,
 * et donner les présences reçues au widget d'écran d'accueil.
 */
object PresenceAnnouncer {
  /** Récepteur du plugin home-widget (même appli, non exporté). */
  private const val WIDGET_RECEIVER = "fr.eveetlucas.homewidget.PresenceReceiver"
  const val ACTION_PRESENCE = "fr.eveetlucas.homewidget.ACTION_PRESENCE"

  /** Réglages d'envoi complets : clé lisible, prénom et au moins une adresse. */
  fun canPush(config: PushConfig): Boolean =
    PushCrypto.decodeKey(config.key) != null &&
      config.who.isNotBlank() &&
      PushSender.validTargets(config.targets).isNotEmpty()

  /**
   * Envoie [event] aux autres appareils (sans prénom, ils refuseraient le
   * message : rien n'est envoyé). [done] est toujours appelé.
   */
  fun push(context: Context, event: PendingArrival, done: () -> Unit) {
    val config = GeofenceStore.pushConfig(context)
    if (!canPush(config)) {
      done()
      return
    }
    val message = ArrivalMessage(
      event.id,
      config.who,
      config.device,
      event.at,
      event.kind,
      event.inside,
      event.place,
      event.placeKind
    )
    try {
      PushSender.send(config, message, done)
    } catch (e: Exception) {
      Log.w(TAG, "Envoi ntfy impossible", e)
      done()
    }
  }

  /**
   * Présence d'un autre reçue par ntfy : le widget se met à jour tout de
   * suite, même appli fermée. Intent explicite (classe nommée) : seule
   * cette appli le reçoit, et Android 8+ ne le bloque pas.
   */
  fun toWidget(context: Context, message: ArrivalMessage) {
    val intent = Intent(ACTION_PRESENCE)
      .setClassName(context.packageName, WIDGET_RECEIVER)
      .putExtra("who", message.who)
      .putExtra("inside", message.inside)
      .putExtra("place", message.place)
      .putExtra("placeKind", message.placeKind)
      .putExtra("at", message.at)
    try {
      context.sendBroadcast(intent)
    } catch (e: Exception) {
      Log.w(TAG, "Widget non prévenu de la présence reçue", e)
    }
  }
}
