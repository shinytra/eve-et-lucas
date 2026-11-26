// Appareils appairés, appairage et état de la synchro.

(() => {
  const tauri = window.__TAURI__;
  if (!tauri?.core?.invoke) return;
  const { invoke } = tauri.core;
  const $ = (id) => document.getElementById(id);
  const list = $("devices");
  const errorBox = $("devices-error");
  const pairing = $("pairing");
  const statusLine = $("sync-status");

  function showError(message) {
    errorBox.textContent = message ?? "";
    errorBox.hidden = !message;
  }

  function formatTime(ms) {
    return new Date(ms).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" });
  }

  async function refreshDevices() {
    let devices;
    try {
      devices = await invoke("list_devices");
    } catch (e) {
      showError(String(e));
      return;
    }
    list.replaceChildren(
      ...devices.map((device) => {
        const li = document.createElement("li");
        li.className = "device";
        const name = document.createElement("span");
        name.textContent = device.this_device ? `${device.name} (cet appareil)` : device.name;
        li.append(name);
        if (!device.this_device) {
          const remove = document.createElement("button");
          remove.textContent = "Retirer";
          remove.className = "danger";
          remove.addEventListener("click", async () => {
            remove.disabled = true;
            try {
              await invoke("remove_device", { id: device.id });
            } catch (e) {
              showError(String(e));
            }
            await refreshDevices();
          });
          li.append(remove);
        } else {
          $("device-name").value = device.name;
        }
        return li;
      }),
    );
    const others = devices.length - 1;
    if (others <= 0) {
      statusLine.textContent = "Aucun autre appareil appairé pour l'instant.";
    } else if (statusLine.dataset.reached === undefined) {
      statusLine.textContent = `${others} appareil(s) appairé(s), synchro dans un instant…`;
    }
  }

  $("pair-start").addEventListener("click", async () => {
    showError(null);
    try {
      const view = await invoke("start_pairing");
      // SVG produit par l'appli elle-même (crate qrcode), pas par un tiers.
      $("pair-qr").innerHTML = view.qr_svg;
      $("pair-code").value = view.code;
      pairing.hidden = false;
    } catch (e) {
      showError(String(e));
    }
  });

  $("pair-copy").addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText($("pair-code").value);
    } catch {
      $("pair-code").select();
    }
  });

  $("pair-cancel").addEventListener("click", async () => {
    pairing.hidden = true;
    $("pair-qr").replaceChildren();
    try {
      await invoke("cancel_pairing");
    } catch {}
  });

  $("join-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    showError(null);
    const button = $("join-submit");
    const code = $("join-code").value.trim();
    if (!code) return;
    button.disabled = true;
    statusLine.textContent = "Appairage en cours…";
    try {
      statusLine.textContent = await invoke("join_pairing", { code });
      $("join-code").value = "";
    } catch (e) {
      showError(String(e));
      statusLine.textContent = "";
    } finally {
      button.disabled = false;
    }
    await refreshDevices();
  });

  // Sur téléphone : scanner le QR code affiché par le PC.
  if (/Android|iPhone|iPad/i.test(navigator.userAgent)) {
    const scan = $("join-scan");
    scan.hidden = false;
    scan.addEventListener("click", async () => {
      showError(null);
      try {
        let permission = await invoke("plugin:barcode-scanner|check_permissions");
        if (permission?.camera !== "granted") {
          permission = await invoke("plugin:barcode-scanner|request_permissions");
        }
        if (permission?.camera !== "granted") {
          showError("L'appareil photo est nécessaire pour scanner le QR code. Tu peux aussi coller le code.");
          return;
        }
        const result = await invoke("plugin:barcode-scanner|scan", { windowed: false, formats: ["QR_CODE"] });
        if (result?.content) {
          $("join-code").value = result.content;
          $("join-form").requestSubmit();
        }
      } catch (e) {
        showError(`Scan impossible : ${e}`);
      }
    });
  }

  $("rename-form").addEventListener("submit", async (event) => {
    event.preventDefault();
    showError(null);
    try {
      await invoke("rename_this_device", { name: $("device-name").value });
    } catch (e) {
      showError(String(e));
    }
    await refreshDevices();
  });

  $("sync-now").addEventListener("click", () => invoke("sync_now").catch((e) => showError(String(e))));

  function showStatus(payload) {
    if (!payload) return;
    if (payload.error) {
      statusLine.textContent = payload.error;
      return;
    }
    if (payload.total === 0) {
      statusLine.textContent = "Aucun autre appareil appairé pour l'instant.";
      return;
    }
    statusLine.dataset.reached = String(payload.reached);
    let text = `Dernière synchro à ${formatTime(payload.at)} : ${payload.reached} appareil(s) joint(s) sur ${payload.total}.`;
    const absent = payload.failures.filter((f) => f.unreachable).map((f) => f.device);
    if (absent.length) text += ` Injoignable(s) pour l'instant : ${absent.join(", ")}.`;
    statusLine.textContent = text;
    // Une vraie erreur (disque, protocole) est signalée à part.
    const errors = payload.failures.filter((f) => !f.unreachable);
    showError(errors.length ? errors.map((f) => `${f.device} : ${f.message}`).join("\n") : null);
  }

  tauri.event?.listen("sync-status", ({ payload }) => showStatus(payload));

  tauri.event?.listen("pairing-used", ({ payload: accepted }) => {
    pairing.hidden = true;
    $("pair-qr").replaceChildren();
    statusLine.textContent = accepted
      ? "Nouvel appareil appairé."
      : "Le code d'appairage a été utilisé sans succès ; crée-en un nouveau si besoin.";
  });

  tauri.event?.listen("docs-changed", ({ payload }) => {
    if (payload?.includes("devices")) refreshDevices();
  });

  refreshDevices().then(() => invoke("sync_status").then(showStatus).catch(() => {}));
})();
