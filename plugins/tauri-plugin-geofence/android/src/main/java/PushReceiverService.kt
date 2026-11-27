package fr.eveetlucas.geofence

import android.util.Log
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage

/**
 * Réception UnifiedPush (bibliothèque `org.unifiedpush.android:connector`).
 *
 * Le récepteur exporté est celui de la bibliothèque (`MessagingReceiverImpl`) :
 * le distributeur (l'appli ntfy) est une autre appli, n'importe quelle appli
 * pourrait donc nous envoyer quelque chose. Il transmet les évènements à ce
 * service, non exporté. Rien n'est cru sans avoir été déchiffré et
 * authentifié (AES-GCM) avec la clé commune.
 *
 * Fonctionne appli fermée : Android démarre le processus pour le récepteur.
 */
class PushReceiverService : PushService() {

  companion object {
    /** Une arrivée plus ancienne (ou annoncée dans le futur) ne sonne plus. */
    const val RING_WITHIN_MS = 15 * 60 * 1000L

    /** Une présence plus ancienne arrivera de toute façon par la synchro. */
    const val PRESENCE_WITHIN_MS = 12 * 60 * 60 * 1000L
  }

  override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
    Log.i(TAG, "Nouvelle adresse UnifiedPush reçue")
    GeofenceStore.setPushEndpoint(this, endpoint.url)
    GeofencePlugin.notifyPushEndpoint(endpoint.url)
  }

  override fun onMessage(message: PushMessage, instance: String) {
    try {
      handleMessage(message.content)
    } catch (e: Exception) {
      // Jamais bloquant : un message bizarre ne doit pas arrêter l'appli.
      Log.w(TAG, "Message UnifiedPush ignoré (erreur inattendue)", e)
    }
  }

  override fun onRegistrationFailed(reason: FailedReason, instance: String) {
    Log.w(TAG, "Enregistrement UnifiedPush refusé : $reason")
  }

  override fun onUnregistered(instance: String) {
    Log.w(TAG, "Plus enregistré auprès du distributeur UnifiedPush")
    GeofenceStore.setPushEndpoint(this, null)
    // Adresse vide : l'appli la retire du document partagé, pour que les
    // autres n'envoient plus dans le vide.
    GeofencePlugin.notifyPushEndpoint("")
  }

  /**
   * Nouveauté (note ajoutée, tâche faite…) : mêmes règles que
   * `Event::should_show` du cœur Rust. Rien n'est gardé pour l'appli : la
   * synchro apportera le changement lui-même.
   */
  private fun handleEvent(event: EventMessage) {
    val config = GeofenceStore.pushConfig(this)
    if (event.kind !in config.notify) {
      Log.i(TAG, "Nouveauté ignorée : type « ${event.kind} » coupé sur ce téléphone")
      return
    }
    if (event.device == config.device) return
    // Liste connue (appli rouverte depuis la mise à jour) : seulement des
    // appareils appairés. Un appareil retiré garde peut-être l'ancienne clé.
    if (config.devices.isNotEmpty() && event.device !in config.devices) {
      Log.w(TAG, "Nouveauté ignorée : appareil qui n'est plus appairé")
      return
    }
    val owner = config.owner.trim()
    if (owner.isNotEmpty() && owner.equals(event.who.trim(), ignoreCase = true)) {
      Log.i(TAG, "Nouveauté ignorée : faite par la personne qui porte ce téléphone")
      return
    }
    val age = System.currentTimeMillis() - event.at
    if (age > EventPayload.SHOW_WITHIN_MS || age < -EventPayload.CLOCK_SKEW_MS) {
      Log.w(TAG, "Nouveauté ignorée : trop ancienne (ou horloge décalée)")
      return
    }
    if (!GeofenceStore.claimEvent(this, event.id)) return
    NewsNotifier.show(this, event)
  }

  private fun handleMessage(content: ByteArray) {
    val key = PushCrypto.decodeKey(GeofenceStore.pushConfig(this).key)
    if (key == null) {
      Log.w(TAG, "Message ntfy ignoré : pas de clé configurée")
      return
    }
    val plain = PushCrypto.decrypt(key, String(content, Charsets.UTF_8))
    if (plain == null) {
      Log.w(TAG, "Message ntfy ignoré : illisible ou pas chiffré avec notre clé")
      return
    }
    val text = String(plain, Charsets.UTF_8)
    val arrival = ArrivalPayload.decode(text)
    if (arrival == null) {
      val event = EventPayload.decode(text)
      if (event == null) {
        Log.w(TAG, "Message ntfy ignoré : ni arrivée ni nouveauté (ou version inconnue)")
      } else {
        handleEvent(event)
      }
      return
    }
    // Relue ici : la configuration peut avoir changé depuis la clé.
    val config = GeofenceStore.pushConfig(this)
    if (arrival.device == config.device) {
      Log.w(TAG, "Message ntfy ignoré : c'est notre propre arrivée")
      return
    }
    if (arrival.kind == KIND_PRESENCE) {
      handlePresence(arrival)
      return
    }
    val age = System.currentTimeMillis() - arrival.at
    if (age > RING_WITHIN_MS || age < -RING_WITHIN_MS) {
      Log.w(TAG, "Message ntfy ignoré : arrivée trop ancienne (ou horloge décalée)")
      return
    }
    if (!GeofenceStore.claimRing(this, arrival.id)) {
      Log.w(TAG, "Message ntfy ignoré : arrivée déjà signalée")
      return
    }
    // D'abord enregistrer (pour `peek_received`), puis prévenir.
    GeofenceStore.addReceived(this, arrival)
    ArrivalNotifier.showReceived(this, arrival)
    PresenceAnnouncer.toWidget(this, arrival)
    GeofencePlugin.notifyReceived(arrival)
  }

  /**
   * Changement de lieu de l'autre (entrée dans un lieu non annoncé, sortie) :
   * rien ne sonne, l'appli et le widget sont mis à jour.
   */
  private fun handlePresence(presence: ArrivalMessage) {
    val age = System.currentTimeMillis() - presence.at
    if (age > PRESENCE_WITHIN_MS || age < -RING_WITHIN_MS) {
      Log.w(TAG, "Présence ignorée : trop ancienne (ou horloge décalée)")
      return
    }
    if (!GeofenceStore.addReceived(this, presence)) return
    PresenceAnnouncer.toWidget(this, presence)
    GeofencePlugin.notifyReceived(presence)
  }
}
