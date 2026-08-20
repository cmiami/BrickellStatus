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
    detail: 'Checking the local transport…'
  });
  let deviceCandidates = $state<DisplayDeviceCandidate[]>([]);
  let firmware = $state<FirmwareStatus | null>(null);
  let reflashing = $state(false);

  const transports: Array<{
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

  // Whatever board answered. Nothing here asks which one it is, and nothing
  // stores an answer: the panel is a property of the hardware on the desk.
  const panel = $derived(deviceStatus.panel ?? 'e213');
  const geometry = $derived(PANEL_GEOMETRY[panel]);
  const detected = $derived(Boolean(deviceStatus.panel));

  // The board reports which panel it is, but not which revision of that panel
  // it carries -- same controller, same wiring, nothing to read back. So when a
  // board has more than one build, the only way to settle it is to write one
  // and look at the screen. That makes "write the other one" a permanent
  // control here, not a prompt that disappears once the flash succeeds.
  const otherBuild = $derived(
    firmware?.variants.find(
      (variant) => variant.panel === panel && variant.id !== firmware?.recommendedVariantId
    ) ?? null
  );

  const epaperOutput = $derived($snapshot?.outputs.find((output) => output.id === 'epaper'));
  const settingsDirty = $derived(
    Boolean($preferences && JSON.stringify(draft.display) !== JSON.stringify($preferences.display))
  );

  onMount(() => {
    let disposed = false;
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
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'Device scan failed.' });
    } finally {
      deviceBusy = null;
    }
  }

  async function connectDevice(device: DisplayDeviceCandidate) {
    deviceBusy = 'connect';
    deviceStatus = {
      state: 'connecting',
      transport: device.transport,
      deviceName: device.name,
      detail: `Opening ${device.transport === 'ble' ? 'Bluetooth LE' : 'USB'} transport…`
    };
    try {
      draft.display.transport = device.transport;
      if (device.transport === 'usb') draft.display.serialPort = device.id.replace(/^usb:/, '');
      const saved = await persistPreferences($state.snapshot(draft));
      if (!saved.ok) {
        deviceStatus = { state: 'error', transport: device.transport, deviceName: device.name, detail: saved.message };
        return;
      }
      deviceStatus = await connectDisplayDevice(device.id, device.transport);
      notice.set({
        ok: deviceStatus.state === 'connected',
        message: deviceStatus.state === 'connected' ? `${deviceStatus.detail} This device is now the saved display route.` : deviceStatus.detail
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

  async function flashOtherBuild() {
    if (!firmware?.port || !otherBuild) return;
    reflashing = true;
    try {
      await invoke('flash_firmware', { variantId: otherBuild.id, port: firmware.port });
      firmware = await invoke<FirmwareStatus>('get_firmware_status');
      notice.set({ ok: true, message: `Wrote ${otherBuild.label}. Look at the panel: if it is still blank or scrambled, write the other one back.` });
    } catch (error) {
      notice.set({ ok: false, message: error instanceof Error ? error.message : 'The panel build could not be written.' });
    } finally {
      reflashing = false;
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
          ? `${geometry.width} × ${geometry.height} monochrome frames over USB serial or Bluetooth Low Energy.`
          : 'Monochrome frames over USB serial or Bluetooth Low Energy. The board names its own panel when it connects.'}
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
                    ? 'Transport unavailable'
                    : deviceStatus.state === 'error'
                      ? "Can't reach the panel"
                      : 'Not connected'}
            </h3>
            <p>{deviceStatus.detail}</p>
          </div>
          <div class="status-facts">
            <!-- No invented fallback. This used to print "InkDock <panel>",
                 which is another project's name and, once boards started
                 advertising their own, was wrong on every board as well as
                 unrelated to what the picker would show. -->
            <span>{deviceStatus.deviceName ?? 'No panel connected'}</span>
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
            {deviceBusy === 'scan' ? 'Looking' : settingsDirty ? 'Save your changes first' : 'Find my panel'}
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

      <div class="timing-register">
        <label class="field"><span>Seconds per frame</span><input type="number" min="10" max="180" bind:value={draft.display.dwellSeconds} /><small class="field-note">How long each frame stays up.</small></label>
        <label class="field"><span>Full refresh every</span><input type="number" min="1" max="100" bind:value={draft.display.fullRefreshEvery} /><small class="field-note">Frames between full wipes that clear ghosting.</small></label>
      </div>

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

      {#if otherBuild}
        <div class="panel-rescue">
          <div>
            <strong>Screen blank or scrambled?</strong>
            <span>Two versions of this panel exist and the board cannot tell them apart. Write the other one and look at the screen again.</span>
          </div>
          <button class="secondary-action" onclick={flashOtherBuild} disabled={reflashing || !firmware?.port}>
            {reflashing ? 'Writing…' : `Write ${otherBuild.label}`}
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
        <div class="empty-sheet" role="status"><h3>No live frame yet</h3><p>Refresh the real sources before rendering a frame.</p></div>
      {/if}
    </aside>
  </div>
</section>
