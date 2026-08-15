import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it } from 'vitest';

import { preferences, snapshot } from '$lib/state';
import type { AppPreferences } from '$lib/types';

import OutputsPage from './+page.svelte';

function preferencesFixture(): AppPreferences {
  return {
    unitSystem: 'imperial',
    areas: [],
    profile: {
      id: 'default',
      name: 'Bridge desk',
      preset: 'bridge_first',
      homeChannelId: 'bridge.brickell',
      quietHours: {
        enabled: true,
        start: '22:00',
        end: '06:30',
        timeZone: 'America/New_York',
        bypassEmergency: true
      },
      channels: [
        {
          id: 'bridge.brickell',
          kind: 'bridge',
          title: 'Brickell Avenue',
          enabled: true,
          presence: 'home',
          interruptPreset: 'recommended',
          destinations: ['epaper', 'desktop'],
          maxAgeMinutes: 5,
          maxItems: 1,
          rotationSeconds: 30,
          scope: { bridge: 'Brickell Avenue Bridge' }
        }
      ]
    },
    ais: {
      enabled: false,
      provider: 'aisstream',
      apiKeyConfigured: false,
      radiusKilometers: 8
    },
    display: {
      transport: 'preview',
      serialPort: 'auto',
      bleName: 'Tender E213',
      dwellSeconds: 30,
      returnHomeAfter: 2,
      fullRefreshEvery: 10
    },
    whatsapp: {
      enabled: false,
      phoneNumberId: '',
      recipient: '',
      graphVersion: 'v23.0',
      templateName: 'bridge_status_update',
      languageCode: 'en_US',
      tokenConfigured: false,
      consent: 'not_recorded',
      consentRecipient: null,
      consentRecordedAtMillis: null
    }
  };
}

afterEach(() => {
  cleanup();
  preferences.set(null);
  snapshot.set(null);
});

describe('WhatsApp recipient consent', () => {
  it('revokes recipient-bound opt-in immediately when the recipient changes', async () => {
    const configured = preferencesFixture();
    configured.whatsapp.enabled = true;
    configured.whatsapp.recipient = '+13055550123';
    configured.whatsapp.consent = 'opted_in';
    configured.whatsapp.consentRecipient = '+13055550123';
    configured.whatsapp.consentRecordedAtMillis = 1_786_741_200_000;
    preferences.set(configured);

    render(OutputsPage);
    const optedIn = screen.getByRole('radio', { name: /Opted in/i });
    expect(optedIn).toHaveAttribute('aria-checked', 'true');

    await fireEvent.input(screen.getByRole('textbox', { name: /Recipient/i }), {
      target: { value: '+13055559999' }
    });

    expect(optedIn).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByRole('radio', { name: /Not recorded/i })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(screen.queryByText(/Opt-in recorded/i)).not.toBeInTheDocument();
  });

  it('binds opt-in to the trimmed current recipient with a capture time', async () => {
    const configured = preferencesFixture();
    configured.whatsapp.recipient = '  +13055550123  ';
    preferences.set(configured);

    render(OutputsPage);
    await fireEvent.click(screen.getByRole('radio', { name: /Opted in/i }));

    expect(screen.getByRole('textbox', { name: /Recipient/i })).toHaveValue('+13055550123');
    expect(screen.getByRole('radio', { name: /Opted in/i })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(screen.getByText(/Editing the recipient revokes this record/i)).toBeInTheDocument();
  });
});

describe('AISStream source desk', () => {
  it('stages the real source gate without claiming the saved worker changed', async () => {
    const configured = preferencesFixture();
    preferences.set(configured);

    render(OutputsPage);

    const sourceGate = screen.getByRole('switch', { name: /AISStream disabled/i });
    expect(sourceGate).toHaveAttribute('aria-checked', 'false');
    expect(screen.getByText(/saved bridge-centered bounding box/i)).toBeInTheDocument();

    await fireEvent.click(sourceGate);

    expect(sourceGate).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByText('Unsaved edit')).toBeInTheDocument();
  });
});

describe('display connection safety', () => {
  it('starts in Preview and explains the unauthenticated BLE frame boundary', () => {
    const configured = preferencesFixture();
    preferences.set(configured);

    render(OutputsPage);

    expect(screen.getByRole('radio', { name: /Render only/i })).toHaveAttribute(
      'aria-checked',
      'true'
    );
    expect(
      screen.getByRole('heading', { name: /Bluetooth frame writes are not authenticated/i })
    ).toBeInTheDocument();
    expect(screen.getByText(/does not prove who sent it/i)).toBeInTheDocument();
  });
});
