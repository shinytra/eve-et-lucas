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
      await invoke("join_pairing", { code });
      $("join-code").value = "";
      statusLine.textContent = "Appairage réussi.";
    } catch (e) {
      showError(String(e));
      statusLine.textContent = "";
    } finally {
      button.disabled = false;
    }
    await refreshDevices();
  });

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

  tauri.event?.listen("sync-status", ({ payload }) => {
    if (payload.error) {
      statusLine.textContent = payload.error;
    } else if (payload.total === 0) {
      statusLine.textContent = "Aucun autre appareil appairé pour l'instant.";
    } else {
      statusLine.dataset.reached = String(payload.reached);
      statusLine.textContent = `Dernière synchro à ${formatTime(payload.at)} : ${payload.reached} appareil(s) joint(s) sur ${payload.total}.`;
    }
  });

  tauri.event?.listen("docs-changed", ({ payload }) => {
    if (payload?.includes("devices")) refreshDevices();
  });

  refreshDevices();
})();
