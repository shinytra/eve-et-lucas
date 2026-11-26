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
    val arrival = ArrivalPayload.decode(String(plain, Charsets.UTF_8))
    if (arrival == null) {
      Log.w(TAG, "Message ntfy ignoré : pas une arrivée (ou version inconnue)")
      return
    }
    // Relue ici : la configuration peut avoir changé depuis la clé.
    if (arrival.device == GeofenceStore.pushConfig(this).device) {
      Log.w(TAG, "Message ntfy ignoré : c'est notre propre arrivée")
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
    GeofencePlugin.notifyReceived(arrival)
  }
}
