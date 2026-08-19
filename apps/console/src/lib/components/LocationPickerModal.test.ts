import { cleanup, fireEvent, render, screen } from '@testing-library/svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';

import LocationPickerModal from './LocationPickerModal.svelte';

// MapLibre needs WebGL, which jsdom does not have. The map is not what these
// tests are about: the contract is that nothing leaves this dialog until the
// reader confirms it.
vi.mock('$lib/components/LocationMap.svelte', async () => {
  const { default: Stub } = await import('./__fixtures__/LocationMapStub.svelte');
  return { default: Stub };
});

afterEach(cleanup);

function mount() {
  const onconfirm = vi.fn();
  const oncancel = vi.fn();
  render(LocationPickerModal, {
    title: 'Coverage area',
    description: 'Drop a pin.',
    latitude: 25.7699,
    longitude: -80.19005,
    onconfirm,
    oncancel
  });
  return { onconfirm, oncancel };
}

describe('the location picker', () => {
  it('shows the starting coordinate and calls nothing on its own', () => {
    const { onconfirm, oncancel } = mount();
    expect(screen.getByText('25.76990, -80.19005')).toBeInTheDocument();
    expect(screen.getByText(/unchanged/i)).toBeInTheDocument();
    expect(onconfirm).not.toHaveBeenCalled();
    expect(oncancel).not.toHaveBeenCalled();
  });

  // The point of a dialog over an always-open map: moving the pin is looking,
  // not deciding.
  it('stages a move without applying it', async () => {
    const { onconfirm } = mount();
    await fireEvent.click(screen.getByTestId('map-pick'));
    expect(screen.getByText('25.80000, -80.10000')).toBeInTheDocument();
    expect(screen.getByText(/not saved yet/i)).toBeInTheDocument();
    expect(onconfirm).not.toHaveBeenCalled();
  });

  it('hands the staged place back only when confirmed', async () => {
    const { onconfirm } = mount();
    await fireEvent.click(screen.getByTestId('map-pick'));
    await fireEvent.click(screen.getByRole('button', { name: /use this place/i }));
    expect(onconfirm).toHaveBeenCalledWith(25.8, -80.1);
  });

  it('discards a staged move on cancel', async () => {
    const { onconfirm, oncancel } = mount();
    await fireEvent.click(screen.getByTestId('map-pick'));
    await fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(oncancel).toHaveBeenCalled();
    expect(onconfirm).not.toHaveBeenCalled();
  });

  it('closes on Escape so a keyboard reader is never trapped', async () => {
    const { oncancel } = mount();
    await fireEvent.keyDown(window, { key: 'Escape' });
    expect(oncancel).toHaveBeenCalled();
  });
});
