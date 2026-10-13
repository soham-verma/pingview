import { useEffect, useState } from 'react';
import { listen } from '@tauri-apps/api/event';

const INITIAL_RECEIVER = { status: 'searching', port: null, detail: null, msg_rate: 0 };

// The Rust side sends JSON nulls; the map components were written against
// adsb-tar1090's bridge, which omitted missing fields. Normalise here so
// they keep working unchanged. The Ping may report only geometric
// altitude, so that stands in for barometric when it's all there is.
function normalise(ac) {
  const out = {};
  for (const [key, value] of Object.entries(ac)) {
    if (value !== null) out[key] = value;
  }
  out.alt_baro = ac.alt_baro ?? ac.alt_geom ?? undefined;
  out.alt_source = ac.alt_baro != null ? 'baro' : ac.alt_geom != null ? 'geom' : undefined;
  return out;
}

// Live aircraft pushed by the backend once a second over the "aircraft"
// event. Everything stays in memory; the receiver is the source of truth.
export function useAircraft() {
  const [aircraft, setAircraft] = useState([]);
  const [messages, setMessages] = useState(0);
  const [receiver, setReceiver] = useState(INITIAL_RECEIVER);
  const [net, setNet] = useState('online'); // 'online' | 'offline' | 'local_only'

  useEffect(() => {
    const unlisten = listen('aircraft', ({ payload }) => {
      setAircraft(
        payload.aircraft.filter((ac) => ac.lat != null && ac.lon != null).map(normalise),
      );
      setMessages(payload.messages);
      setReceiver(payload.receiver);
      if (payload.net) setNet(payload.net);
    });
    return () => {
      unlisten.then((stop) => stop());
    };
  }, []);

  return { aircraft, messages, receiver, net };
}
