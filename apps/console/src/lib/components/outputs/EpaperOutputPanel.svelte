<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {
    Bluetooth,
    BluetoothConnected,
    BluetoothSearching,
    Cable,
    Check,
    CircleDotDashed,
    MonitorUp,
    RefreshCw,
    Send,
    Unplug,
    Usb
  } from '@lucide/svelte';

  import EpaperPreview from '$lib/components/EpaperPreview.svelte';
  import {
    connectDisplayDevice,
    disconnectDisplayDevice,
    getDisplayStatus,
    getPlatformCapabilities,
    scanDisplayDevices,
    sendDisplayTestFrame
  } from '$lib/api';
  import { notice, persistPreferences, preferences, snapshot } from '$lib/state';
  import {
    PANEL_GEOMETRY,
    type AppPreferences,
    type DisplayConnectionStatus,
    type DisplayDeviceCandidate,
    type DisplaySettings,
    type FirmwareStatus
  } from '$lib/types';

  let { draft = $bindable() }: { draft: AppPreferences } = $props();

  let displayBusy = $state(false);
  let deviceBusy = $state<'scan' | 'connect' | 'disconnect' | null>(null);
  let deviceStatus = $state<DisplayConnectionStatus>({
    state: 'disconnected',
    transport: null,
    detail: 'Checking the panel connection…'
  });
  let deviceCandidates = $state<DisplayDeviceCandidate[]>([]);
  let firmware = $state<FirmwareStatus | null>(null);
  let reflashingFirmware = $state(false);

  const durableBleName = /^BrickellStatus [0-9A-F]{4}$/;

  function isDurableBleName(name: string): boolean {
    return durableBleName.test(name) && name !== 'BrickellStatus 0000';
  }

  function panelDisplayName(status: DisplayConnectionStatus): string {
    if (status.transport === 'ble') {
      return status.deviceName && isDurableBleName(status.deviceName)
        ? status.deviceName
        : 'BrickellStatus panel';
    }
    if (status.transport === 'usb') return 'USB panel';
    return 'No panel connected';
  }

  // Assume USB until the backend says otherwise, so the register never flickers
  // a smaller set of choices on a desktop that has them all.
  let usbDisplay = $state(true);
  // Writing firmware needs a serial bootloader, which no phone can reach.
  let flashingSupported = $state(true);

  const allTransports: Array<{
    id: DisplaySettings['transport'];
    label: string;
    detail: string;
    icon: typeof Cable;
  }> = [
    { id: 'preview', label: 'No panel', detail: 'Draw frames on screen only. Nothing is sent to hardware.', icon: MonitorUp },
    { id: 'auto', label: 'Whichever works', detail: 'Use the panel you picked, over USB or Bluetooth.', icon: Cable },
    { id: 'usb', label: 'USB cable', detail: 'Only talk to the panel over the cable.', icon: Usb },
    { id: 'ble', label: 'Bluetooth', detail: 'Only talk to the panel over Bluetooth.', icon: Bluetooth }
  ];

  // On a phone there is no serial port to open, so "USB only" would be a
  // setting that can only fail, and "Automatic" would promise a fallback with
  // nothing to fall back from. Bluetooth is the whole story there.
  const transports = $derived(
    usbDisplay ? allTransports : allTransports.filter((transport) => transport.id !== 'usb' && transport.id !== 'auto')
  );
  const transportProse = $derived(
    usbDisplay ? 'USB serial or Bluetooth Low Energy' : 'Bluetooth Low Energy'
  );

  // Whatever board answered. Nothing here asks which one it is, and nothing
  // stores an answer: the panel is a property of the hardware on the desk.
  const panel = $derived(deviceStatus.panel ?? 'e213');
  const geometry = $derived(PANEL_GEOMETRY[panel]);
  const detected = $derived(Boolean(deviceStatus.panel));

  // E213's two controller images are internal recovery candidates, not two
  // panel choices. This single reflash action starts with the recommended image;
  // the backend requires READY and tries the other E213 controller once when
  // needed, without treating retained pixels as proof that firmware is alive.
  const reflashBuild = $derived(
    !flashingSupported || firmware?.requirement.state === 'deviceNewer'
      ? null
      : firmware?.variants.find((variant) => variant.id === firmware?.recommendedVariantId) ??
        firmware?.variants.find((variant) => variant.panel === panel) ??
        null
  );

  const epaperOutput = $derived($snapshot?.outputs.find((output) => output.id === 'epaper'));
  const settingsDirty = $derived(
    Boolean($preferences && JSON.stringify(draft.display) !== JSON.stringify($preferences.display))
  );

  onMount(() => {
    let disposed = false;
    void getPlatformCapabilities()
      .then((capabilities) => {
        if (disposed) return;
        usbDisplay = capabilities.usbDisplay;
        flashingSupported = capabilities.firmwareFlashing;
        // A preference carried over from a desktop install would otherwise
        // leave the phone pointed at a transport it cannot open.
        if (!capabilities.usbDisplay && (draft.display.transport === 'usb' || draft.display.transport === 'auto')) {
          draft.display.transport = 'ble';
        }
      })
      .catch(() => {
        // Capability reporting is an affordance, not a requirement; leaving the
        // full register in place matches how this panel behaved before.
      });
    const refresh = async () => {
      try {
        const status = await getDisplayStatus();
        if (!disposed) deviceStatus = status;
      } catch (error) {
        if (!disposed) {
          deviceStatus = {
            state: 'error',
            transport: null,
            detail: error instanceof Error ? error.message : 'Display status is unavailable.'
          };
        }
      }
    };
    const refreshFirmware = async () => {
      try {
        const status = await invoke<FirmwareStatus>('get_firmware_status');
        if (!disposed) firmware = status;
      } catch {
        // No Tauri bridge, or no bundled firmware: the reflash control simply
        // does not appear rather than showing an error nobody can act on.
        if (!disposed) firmware = null;
      }
    };
    void refresh();
    void refreshFirmware();
    const timer = window.setInterval(() => void refresh(), 5_000);
    return () => {
      disposed = true;
      window.clearInterval(timer);
    };
  });

  async function sendFrame() {
    displayBusy = true;
    try {
      notice.set(await sendDisplayTestFrame());
      deviceStatus = await getDisplayStatus();
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The display frame could not be sent.' });
    } finally {
      displayBusy = false;
    }
  }

  async function scanDevices() {
    deviceBusy = 'scan';
    deviceCandidates = [];
    try {
      deviceCandidates = await scanDisplayDevices();
      if (!deviceCandidates.length) {
        notice.set({ ok: false, message: 'No panel found. Switch the board on, allow Bluetooth, or plug in the USB cable and look again.' });
      }
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The panel search failed.' });
    } finally {
      deviceBusy = null;
    }
  }

  async function connectDevice(device: DisplayDeviceCandidate) {
    const remembersBleRoute = device.transport === 'ble' && isDurableBleName(device.name);
    deviceBusy = 'connect';
    deviceStatus = {
      state: 'connecting',
      transport: device.transport,
      deviceName: device.name,
      detail: `Connecting over ${device.transport === 'ble' ? 'Bluetooth' : 'USB'}…`
    };
    try {
      draft.display.transport = device.transport;
      if (device.transport === 'usb') {
        draft.display.serialPort = device.id.replace(/^usb:/, '');
      } else {
        // A platform peripheral ID is safe for this live app session. Only the
        // firmware's board-specific name is safe after restart; old generic or
        // missing names can identify a different panel on the same desk.
        draft.display.bleName = remembersBleRoute ? device.name : '';
      }
      const saved = await persistPreferences($state.snapshot(draft));
      if (!saved.ok) {
        deviceStatus = { state: 'error', transport: device.transport, deviceName: device.name, detail: saved.message };
        return;
      }
      deviceStatus = await connectDisplayDevice(device.id, device.transport);
      notice.set({
        ok: deviceStatus.state === 'connected',
        message: deviceStatus.state === 'connected'
          ? remembersBleRoute || device.transport === 'usb'
            ? `${deviceStatus.detail} This is now your saved panel.`
            : `${deviceStatus.detail} Connected for this session. The panel does not provide a unique name, so the app cannot reconnect to it automatically.`
          : deviceStatus.detail
      });
    } catch (error) {
      deviceStatus = {
        state: 'error',
        transport: device.transport,
        deviceName: device.name,
        detail: error instanceof Error ? error.message : 'Could not connect to the display.'
      };
      notice.set({ ok: false, message: deviceStatus.detail });
    } finally {
      deviceBusy = null;
    }
  }

  async function reflashPanelFirmware() {
    if (!firmware?.port || !reflashBuild) return;
    reflashingFirmware = true;
    try {
      await invoke('flash_firmware', { variantId: reflashBuild.id, port: firmware.port });
      firmware = await invoke<FirmwareStatus>('get_firmware_status');
      notice.set({ ok: true, message: 'Panel firmware was written and verified. The saved connection now follows this panel.' });
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'Panel firmware could not be reflashed.' });
    } finally {
      reflashingFirmware = false;
    }
  }

  async function disconnectDevice() {
    deviceBusy = 'disconnect';
    try {
      deviceStatus = await disconnectDisplayDevice();
      notice.set({ ok: true, message: deviceStatus.detail });
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'Disconnect failed.' });
    } finally {
      deviceBusy = null;
    }
  }
</script>

<section class="output-band epaper-band" aria-labelledby="epaper-heading">
  <header class="band-heading">
    <div class="route-mark"><MonitorUp size={26} strokeWidth={1.45} aria-hidden="true" /></div>
    <div>
      <h2 id="epaper-heading">
        {detected ? `Heltec ${geometry.label} e-paper` : 'Heltec e-paper'}
      </h2>
      <p>
        {detected
          ? `${geometry.width} × ${geometry.height} monochrome frames over ${transportProse}.`
          : `Monochrome frames over ${transportProse}. The board names its own panel when it connects.`}
      </p>
    </div>
    <span class="status-word" data-state={deviceStatus.state === 'connected' ? 'ready' : deviceStatus.state}>
      {deviceStatus.state}
    </span>
  </header>

  <div class="epaper-work">
    <div class="transport-work">
      <fieldset>
        <legend>How to connect</legend>
        <div class="transport-register">
          {#each transports as transport}
            {@const TransportIcon = transport.icon}
            <button type="button" role="radio" aria-checked={draft.display.transport === transport.id} onclick={() => (draft.display.transport = transport.id)}>
              <TransportIcon size={20} strokeWidth={1.5} aria-hidden="true" />
              <span><strong>{transport.label}</strong><small>{transport.detail}</small></span>
              {#if draft.display.transport === transport.id}<Check size={17} aria-hidden="true" />{/if}
            </button>
          {/each}
        </div>
      </fieldset>

      <section class="device-setup" aria-labelledby="device-setup-heading">
        <header class="device-status" data-state={deviceStatus.state}>
          <div class="status-glyph">
            {#if deviceStatus.state === 'connected'}
              {#if deviceStatus.transport === 'ble'}<BluetoothConnected size={25} strokeWidth={1.45} aria-hidden="true" />{:else}<Usb size={25} strokeWidth={1.45} aria-hidden="true" />{/if}
            {:else if deviceStatus.state === 'connecting'}
              <BluetoothSearching size={25} strokeWidth={1.45} aria-hidden="true" />
            {:else}
              <CircleDotDashed size={25} strokeWidth={1.45} aria-hidden="true" />
            {/if}
          </div>
          <div>
            <h3 id="device-setup-heading">
              {deviceStatus.state === 'connected'
                ? `${deviceStatus.transport === 'ble' ? 'Bluetooth LE' : 'USB'} connected`
                : deviceStatus.state === 'connecting'
                  ? 'Opening connection'
                  : deviceStatus.state === 'unavailable'
                    ? 'Connection unavailable'
                    : deviceStatus.state === 'error'
                      ? "Can't reach the panel"
                      : 'Not connected'}
            </h3>
            <p>{deviceStatus.detail}</p>
          </div>
          <div class="status-facts">
            <!-- The picker may expose a legacy advertisement so it can be
                 selected by exact session ID. Once connected, only the current
                 board-specific identity deserves to become user-facing copy. -->
            <span>{panelDisplayName(deviceStatus)}</span>
            <strong>{deviceStatus.transport?.toUpperCase() ?? 'NO LINK'}</strong>
            {#if deviceStatus.lastAckAt}<small>Last ACK {new Date(deviceStatus.lastAckAt).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</small>{/if}
          </div>
          {#if deviceStatus.state === 'connected'}
            <button class="disconnect-action" onclick={disconnectDevice} disabled={deviceBusy !== null}><Unplug size={16} aria-hidden="true" /> Disconnect</button>
          {/if}
        </header>

        <div class="find-panel">
          <p>Plug the panel in or switch it on, then look for it. Each board shows its own four-character code on screen, so you can tell two of them apart.</p>
          <button class="scan-action" onclick={scanDevices} disabled={deviceBusy !== null || draft.display.transport === 'preview' || settingsDirty}>
            <RefreshCw size={16} class={deviceBusy === 'scan' ? 'spinning' : undefined} aria-hidden="true" />
            {deviceBusy === 'scan' ? 'Looking' : settingsDirty ? 'Applying changes…' : 'Find my panel'}
          </button>
        </div>


        {#if deviceCandidates.length}
          <div class="device-candidates" aria-label="Discovered display devices">
            <header>
              <span>Panels found — match the code on the screen</span>
              <strong>{deviceCandidates.length.toString().padStart(2, '0')}</strong>
            </header>
            {#each deviceCandidates as device (device.id)}
              <article>
                <div class="candidate-transport">{#if device.transport === 'ble'}<Bluetooth size={20} strokeWidth={1.45} aria-hidden="true" />{:else}<Usb size={20} strokeWidth={1.45} aria-hidden="true" />{/if}<span>{device.transport === 'ble' ? 'BLE' : 'USB'}</span></div>
                <div><strong>{device.name}</strong><small>{device.detail}{device.signalStrength !== undefined ? ` · ${device.signalStrength} dBm` : ''}</small></div>
                <button onclick={() => connectDevice(device)} disabled={deviceBusy !== null || deviceStatus.state === 'connected' || settingsDirty}>{deviceBusy === 'connect' ? 'Connecting' : 'Connect'}</button>
              </article>
            {/each}
          </div>
        {/if}

      </section>

      <p class="automatic-cadence">Current items advance automatically. Urgent notices move to the front, and the panel handles full refreshes as maintenance.</p>

      <!-- Which way up the board is screwed down. The preview stays upright
           either way: it shows what the reader sees, and the reader is looking
           at the panel the right way up whichever way it is mounted. -->
      <fieldset class="orientation">
        <legend>Which way up</legend>
        <div role="radiogroup" aria-label="Panel orientation">
          {#each [{ id: 'upright', label: 'Upright' }, { id: 'inverted', label: 'Upside down' }] as option (option.id)}
            <button
              type="button"
              role="radio"
              aria-checked={draft.display.orientation === option.id}
              class:selected={draft.display.orientation === option.id}
              onclick={() => (draft.display.orientation = option.id as 'upright' | 'inverted')}
            >
              <b>{option.label}</b>
            </button>
          {/each}
        </div>
        <small class="field-note">Pick whichever matches the panel in front of you.</small>
      </fieldset>

      {#if reflashBuild && firmware?.port}
        <div class="panel-rescue">
          <div>
            <strong>Panel firmware</strong>
            <span>Reinstall and verify the firmware bundled with this app. E213 controller selection is automatic.</span>
          </div>
          <button class="secondary-action" onclick={reflashPanelFirmware} disabled={reflashingFirmware}>
            {reflashingFirmware ? 'Reflashing…' : 'Reflash firmware'}
          </button>
        </div>
      {/if}

      <div class="test-line">
        <div><strong>{deviceStatus.state === 'connected' ? 'Connected and ready to test' : (epaperOutput?.detail ?? 'No panel connected')}</strong><span>The test passes only once the panel confirms it got the whole frame.</span></div>
        <button class="secondary-action action-with-icon" onclick={sendFrame} disabled={displayBusy || deviceStatus.state !== 'connected' || settingsDirty}><Send size={16} aria-hidden="true" /> {displayBusy ? 'Sending' : 'Send a test frame'}</button>
      </div>
    </div>

    <aside class="frame-preview">
      {#if $snapshot}
        <EpaperPreview
          decision={$snapshot.decision}
          evidence={$snapshot.evidence}
          connected={deviceStatus.state === 'connected'}
          transport={deviceStatus.transport ?? draft.display.transport}
          {panel}
        />
      {:else}
        <div class="empty-sheet" role="status"><h3>No live frame yet</h3><p>No current data is available for a test frame.</p></div>
      {/if}
    </aside>
  </div>
</section>
