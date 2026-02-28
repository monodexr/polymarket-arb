import useSWR from 'swr';
import type { ArbStatus, Alert, ArbTrade } from '../lib/types';

const fetcher = async (url: string) => {
  const res = await fetch(url + (url.includes('?') ? '&' : '?') + `_t=${Date.now()}`);
  if (!res.ok) throw new Error(`${res.status}`);
  return res.json();
};

export function useStatus() {
  return useSWR<ArbStatus>('/api/status', fetcher, {
    refreshInterval: 2000,
    dedupingInterval: 1000,
  });
}

export function useAlerts() {
  return useSWR<{ alerts: Alert[] }>('/api/alerts', fetcher, {
    refreshInterval: 2000,
    dedupingInterval: 1000,
  });
}

export function useTrades() {
  return useSWR<{ trades: ArbTrade[] }>('/api/trades', fetcher, {
    refreshInterval: 5000,
    dedupingInterval: 3000,
  });
}
