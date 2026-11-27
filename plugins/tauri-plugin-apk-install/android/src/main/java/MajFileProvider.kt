package fr.eveetlucas.apkinstall

import androidx.core.content.FileProvider

/**
 * FileProvider propre au plugin : le manifeste de l'appli ne peut déclarer
 * qu'une fois `androidx.core.content.FileProvider`, peut-être déjà pris par
 * un autre plugin. Ne partage que `cache/maj/` (voir `apk_install_paths.xml`).
 */
class MajFileProvider : FileProvider()
