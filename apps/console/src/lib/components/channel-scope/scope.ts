import type { ChannelPreference } from '$lib/types';

export type ChannelChange = (channel: ChannelPreference) => void;

export function scopeText(channel: ChannelPreference, key: string, fallback = ''): string {
  const value = channel.scope[key];
  return typeof value === 'string' ? value : fallback;
}

export function scopeNumber(channel: ChannelPreference, key: string, fallback: number): number {
  const value = channel.scope[key];
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

export function scopeBool(channel: ChannelPreference, key: string, fallback: boolean): boolean {
  const value = channel.scope[key];
  return typeof value === 'boolean' ? value : fallback;
}

export function scopeList(channel: ChannelPreference, key: string): string[] {
  const value = channel.scope[key];
  return Array.isArray(value) ? value.filter((item): item is string => typeof item === 'string') : [];
}

export function setScope(
  channel: ChannelPreference,
  onchange: ChannelChange,
  key: string,
  value: string | number | boolean | string[]
): void {
  onchange({ ...channel, scope: { ...channel.scope, [key]: value } });
}

export function toggleScopeList(
  channel: ChannelPreference,
  onchange: ChannelChange,
  key: string,
  value: string,
  enabled: boolean
): void {
  const current = scopeList(channel, key);
  setScope(channel, onchange, key, enabled ? [...new Set([...current, value])] : current.filter((item) => item !== value));
}
