#!/usr/bin/env bash
# Essai sur l'émulateur : ouvre l'appli, va dans Appareils et lance le scan
# du QR code. Échoue si l'appli plante (exception Java ou plantage natif).
# Usage : essai-scan-android.sh <apk> <dossier de sortie>
set -uo pipefail
apk=$1
out=$2
pkg=fr.eveetlucas.couple
mkdir -p "$out"

capture() {
  adb exec-out screencap -p > "$out/$1.png" 2>/dev/null || true
}

# Touche le premier élément de l'écran dont le texte (ou la description)
# contient $1. L'interface web est visible par l'accessibilité d'Android.
toucher() {
  local texte=$1 essai bounds
  for essai in 1 2 3 4 5 6 7 8 9 10; do
    adb shell uiautomator dump /sdcard/ecran.xml > /dev/null 2>&1
    adb exec-out cat /sdcard/ecran.xml > "$out/ecran.xml" 2>/dev/null
    bounds=$(tr '>' '\n' < "$out/ecran.xml" \
      | grep -E "(text|content-desc)=\"[^\"]*${texte}" \
      | head -n 1 | sed -nE 's/.*bounds="\[([0-9]+),([0-9]+)\]\[([0-9]+),([0-9]+)\]".*/\1 \2 \3 \4/p')
    if [ -n "$bounds" ]; then
      set -- $bounds
      adb shell input tap $(( ($1 + $3) / 2 )) $(( ($2 + $4) / 2 ))
      echo "Touché « $texte »."
      sleep 2
      return 0
    fi
    sleep 2
  done
  echo "« $texte » introuvable à l'écran."
  cp "$out/ecran.xml" "$out/introuvable-$(echo "$texte" | tr -c 'a-zA-Z' '-').xml"
  return 1
}

adb install -r "$apk"
adb shell pm grant "$pkg" android.permission.CAMERA
adb logcat -c
adb logcat -v time > "$out/logcat.txt" 2>&1 &
logcat_pid=$!

adb shell monkey -p "$pkg" -c android.intent.category.LAUNCHER 1 > /dev/null
sleep 15
capture 1-lancement
toucher "Je suis Lucas" || true
capture 2-bienvenue
toucher "Plus" && capture 3-plus
toucher "Appareils" && capture 4-appareils
toucher "Scanner le QR code" || { kill $logcat_pid; exit 1; }
sleep 10
capture 5-scan

kill $logcat_pid 2>/dev/null
pid=$(adb shell pidof "$pkg" | tr -d '\r')
grep -E "FATAL EXCEPTION|AndroidRuntime|Fatal signal|DEBUG   :|tauri|barcode|BarcodeScanner|RustStdoutStderr|Chromium" "$out/logcat.txt" | tail -n 200 > "$out/extrait.txt" || true
if grep -qE "FATAL EXCEPTION|Fatal signal" "$out/logcat.txt" || [ -z "$pid" ]; then
  echo "::error::L'appli a planté au scan du QR code (voir extrait.txt)."
  grep -A40 -E "FATAL EXCEPTION|Fatal signal" "$out/logcat.txt" | head -n 80
  exit 1
fi
echo "L'appli tourne toujours après l'ouverture du scan."
