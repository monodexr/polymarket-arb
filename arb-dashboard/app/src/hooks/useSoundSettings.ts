import { useState, useCallback } from 'react';

export interface SoundChannel {
  id: string;
  label: string;
  enabled: boolean;
  volume: number;
}

const DEFAULTS: SoundChannel[] = [
  { id: 'divergence', label: 'Divergence Tick', enabled: true, volume: 50 },
  { id: 'fill', label: 'Order Fill Bell', enabled: true, volume: 75 },
  { id: 'win', label: 'Converge Chime', enabled: true, volume: 88 },
  { id: 'loss', label: 'Adverse Thud', enabled: true, volume: 60 },
];

const STORAGE_KEY = 'arb-sound-settings';

function loadSettings(): SoundChannel[] {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const parsed = JSON.parse(raw);
      return DEFAULTS.map(d => {
        const saved = parsed.find((s: SoundChannel) => s.id === d.id);
        return saved ? { ...d, enabled: saved.enabled, volume: saved.volume } : d;
      });
    }
  } catch {}
  return DEFAULTS.map(d => ({ ...d }));
}

function saveSettings(channels: SoundChannel[]) {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(channels)); } catch {}
}

export function useSoundSettings() {
  const [channels, setChannels] = useState<SoundChannel[]>(loadSettings);
  const [globalMuted, setGlobalMuted] = useState(() =>
    typeof localStorage !== 'undefined' && localStorage.getItem('arb-muted') === '1'
  );

  const setChannel = useCallback((id: string, update: Partial<Pick<SoundChannel, 'enabled' | 'volume'>>) => {
    setChannels(prev => {
      const next = prev.map(c => c.id === id ? { ...c, ...update } : c);
      saveSettings(next);
      return next;
    });
  }, []);

  const toggleGlobalMute = useCallback(() => {
    setGlobalMuted(prev => {
      const next = !prev;
      localStorage.setItem('arb-muted', next ? '1' : '0');
      if (!next) {
        setChannels(prevCh => {
          const restored = prevCh.map(c => ({ ...c, enabled: true }));
          saveSettings(restored);
          return restored;
        });
      }
      return next;
    });
  }, []);

  const muteAll = useCallback(() => {
    setGlobalMuted(true);
    localStorage.setItem('arb-muted', '1');
    setChannels(prev => {
      const next = prev.map(c => ({ ...c, enabled: false }));
      saveSettings(next);
      return next;
    });
  }, []);

  const getVolume = useCallback((id: string): number => {
    if (globalMuted) return 0;
    const ch = channels.find(c => c.id === id);
    if (!ch || !ch.enabled) return 0;
    return ch.volume / 100;
  }, [channels, globalMuted]);

  return { channels, setChannel, globalMuted, toggleGlobalMute, muteAll, getVolume };
}

export type SoundSettings = ReturnType<typeof useSoundSettings>;
