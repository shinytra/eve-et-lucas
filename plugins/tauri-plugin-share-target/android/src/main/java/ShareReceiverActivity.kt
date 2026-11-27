package fr.eveetlucas.sharetarget

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.util.Log

/**
 * Reçoit un texte du menu Partager (par exemple le lien d'un morceau depuis
 * YouTube Music), le note pour l'appli, ouvre l'appli et se ferme aussitôt.
 * L'appli ajoute ensuite la musique (voir `src/music.rs`), même si elle
 * était fermée : le partage l'attend dans [ShareStore].
 */
class ShareReceiverActivity : Activity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    super.onCreate(savedInstanceState)
    try {
      receive(intent)
    } catch (e: Exception) {
      Log.w(TAG, "Partage non reçu", e)
    }
    finish()
  }

  private fun receive(intent: Intent?) {
    if (intent?.action != Intent.ACTION_SEND) return
    val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()?.trim()
    if (text.isNullOrEmpty()) return
    val subject = intent.getCharSequenceExtra(Intent.EXTRA_SUBJECT)?.toString()?.trim() ?: ""
    val share = ShareStore.add(this, text, subject, System.currentTimeMillis())
    ShareTargetPlugin.notifyShare(share)
    // Ouvre l'appli (ou la ramène devant) sur l'écran des musiques.
    val launch = packageManager.getLaunchIntentForPackage(packageName) ?: return
    launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    startActivity(launch)
  }
}
