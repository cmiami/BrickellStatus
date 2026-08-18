<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Activity,
    Bluetooth,
    BluetoothConnected,
    BluetoothSearching,
    Cable,
    Check,
    CircleDotDashed,
    MonitorUp,
    Power,
    RefreshCw,
    Send,
    ShieldAlert,
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
    type DisplaySettings
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

  const transports: Array<{
    id: DisplaySettings['transport'];
    label: string;
    detail: string;
    icon: typeof Cable;
  }> = [
    { id: 'preview', label: 'Render only', detail: 'Render frames without opening hardware.', icon: MonitorUp },
    { id: 'auto', label: 'Automatic', detail: 'Use a device you explicitly scan for and select.', icon: Cable },
    { id: 'usb', label: 'USB only', detail: 'Native serial with INK1 acknowledgement.', icon: Usb },
    { id: 'ble', label: 'Bluetooth only', detail: 'Direct in-app GATT connection.', icon: Bluetooth }
  ];

  // Whatever board answered. Nothing here asks which one it is, and nothing
  // stores an answer: the panel is a property of the hardware on the desk.
  const panel = $derived(deviceStatus.panel ?? 'e213');
  const geometry = $derived(PANEL_GEOMETRY[panel]);
  const detected = $derived(Boolean(deviceStatus.panel));

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
    void refresh();
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
        notice.set({ ok: false, message: 'No panel transport was discovered. Wake the board, allow Bluetooth, or connect USB and scan again.' });
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
        <legend>Connection strategy</legend>
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
            <span>Live device circuit</span>
            <h3 id="device-setup-heading">
              {deviceStatus.state === 'connected'
                ? `${deviceStatus.transport === 'ble' ? 'Bluetooth LE' : 'USB'} connected`
                : deviceStatus.state === 'connecting'
                  ? 'Opening connection'
                  : deviceStatus.state === 'unavailable'
                    ? 'Transport unavailable'
                    : deviceStatus.state === 'error'
                      ? 'Connection needs attention'
                      : 'Ready to discover'}
            </h3>
            <p>{deviceStatus.detail}</p>
          </div>
          <div class="status-facts">
            <span>{deviceStatus.deviceName ?? `InkDock ${geometry.label}`}</span>
            <strong>{deviceStatus.transport?.toUpperCase() ?? 'NO LINK'}</strong>
            {#if deviceStatus.lastAckAt}<small>Last ACK {new Date(deviceStatus.lastAckAt).toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}</small>{/if}
          </div>
          {#if deviceStatus.state === 'connected'}
            <button class="disconnect-action" onclick={disconnectDevice} disabled={deviceBusy !== null}><Unplug size={16} aria-hidden="true" /> Disconnect</button>
          {/if}
        </header>

        <ol class="pairing-steps">
          <li><span>01</span><Power size={20} strokeWidth={1.45} aria-hidden="true" /><div><strong>Power the board</strong><small>It should show <code>READY / USB + BLE</code>.</small></div></li>
          <li class:complete={deviceCandidates.length > 0 || deviceStatus.state === 'connected'}>
            <span>02</span><BluetoothSearching size={20} strokeWidth={1.45} aria-hidden="true" />
            <div><strong>Discover a route</strong><small>Choose and save a hardware mode, then scan.</small></div>
            <button class="scan-action" onclick={scanDevices} disabled={deviceBusy !== null || draft.display.transport === 'preview' || settingsDirty}>
              <RefreshCw size={16} class={deviceBusy === 'scan' ? 'spinning' : undefined} aria-hidden="true" />
              {deviceBusy === 'scan' ? 'Scanning' : settingsDirty ? 'Save settings first' : 'Scan nearby'}
            </button>
          </li>
          <li class:complete={deviceStatus.state === 'connected' && Boolean(deviceStatus.lastAckAt)}><span>03</span><Activity size={20} strokeWidth={1.45} aria-hidden="true" /><div><strong>Prove the screen</strong><small>Only an <code>ACK INK1</code> marks the route healthy.</small></div></li>
        </ol>

        <aside class="ble-boundary" aria-labelledby="ble-boundary-heading">
          <ShieldAlert size={22} strokeWidth={1.45} aria-hidden="true" />
          <div><h4 id="ble-boundary-heading">Bluetooth frame writes are not authenticated</h4><p>INK1 does not authenticate GATT clients. <code>ACK INK1</code> confirms a complete frame; it does not prove who sent it.</p></div>
        </aside>

        {#if deviceCandidates.length}
          <div class="device-candidates" aria-label="Discovered display devices">
            <header><span>Discovered now</span><strong>{deviceCandidates.length.toString().padStart(2, '0')}</strong></header>
            {#each deviceCandidates as device (device.id)}
              <article>
                <div class="candidate-transport">{#if device.transport === 'ble'}<Bluetooth size={20} strokeWidth={1.45} aria-hidden="true" />{:else}<Usb size={20} strokeWidth={1.45} aria-hidden="true" />{/if}<span>{device.transport === 'ble' ? 'BLE' : 'USB'}</span></div>
                <div><strong>{device.name}</strong><small>{device.detail}{device.signalStrength !== undefined ? ` · ${device.signalStrength} dBm` : ''}</small></div>
                <button onclick={() => connectDevice(device)} disabled={deviceBusy !== null || deviceStatus.state === 'connected' || settingsDirty}>{deviceBusy === 'connect' ? 'Connecting' : 'Connect'}</button>
              </article>
            {/each}
          </div>
        {/if}

        <details class="advanced-transport">
          <summary>Advanced transport selectors</summary>
          <div class="connection-fields">
            <label class="field"><span>USB serial port</span><input bind:value={draft.display.serialPort} placeholder="auto" maxlength="180" disabled={draft.display.transport === 'ble' || draft.display.transport === 'preview'} /><small class="field-note">Use <code>auto</code> to discover the Espressif interface.</small></label>
            <label class="field"><span>Bluetooth device name</span><input bind:value={draft.display.bleName} maxlength="64" disabled={draft.display.transport === 'usb' || draft.display.transport === 'preview'} /><small class="field-note">Any board advertising the INK1 service is found whatever it is named; this only pins discovery to one of them.</small></label>
          </div>
        </details>
      </section>

      <div class="timing-register">
        <label class="field"><span>Routine dwell</span><input type="number" min="10" max="180" bind:value={draft.display.dwellSeconds} /><small class="field-note">Seconds per normal frame.</small></label>
        <label class="field"><span>Return home after</span><input type="number" min="1" max="20" bind:value={draft.display.returnHomeAfter} /><small class="field-note">Routine frames before home.</small></label>
        <label class="field"><span>Full refresh cadence</span><input type="number" min="1" max="100" bind:value={draft.display.fullRefreshEvery} /><small class="field-note">Frames between ghost-clearing refreshes.</small></label>
      </div>

      <div class="test-line">
        <div><strong>{deviceStatus.state === 'connected' ? 'Ready for a physical proof' : (epaperOutput?.detail ?? 'No device report')}</strong><span>A test succeeds only after the board acknowledges the complete frame.</span></div>
        <button class="secondary-action action-with-icon" onclick={sendFrame} disabled={displayBusy || deviceStatus.state !== 'connected' || settingsDirty}><Send size={16} aria-hidden="true" /> {displayBusy ? 'Sending frame' : 'Send current frame'}</button>
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
