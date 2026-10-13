const PROBLEM_TEXT = {
  searching: 'No receiver',
  connecting: 'Connecting…',
  error: 'Receiver error',
};

const DOT_CLASS = {
  searching: 'waiting',
  connecting: 'waiting',
  connected: '',
  no_traffic: 'quiet',
  error: 'disconnected',
};

// Top-left summary. Normally just a dot and the aircraft count; spells the
// problem out when the receiver isn't working. Click for receiver details.
const NET_TEXT = { offline: 'Offline', local_only: 'Local only' };

export default function StatusPill({ receiver, count, net, onClick }) {
  const problem = PROBLEM_TEXT[receiver.status];
  return (
    <button type="button" className="status-pill map-float" onClick={onClick} title="Receiver details">
      <span className={`status-dot ${DOT_CLASS[receiver.status] ?? 'disconnected'}`} />
      {problem ?? `${count} aircraft`}
      {NET_TEXT[net] && (
        <span
          className={`net-state ${net}`}
          title={
            net === 'offline'
              ? 'No internet: online extras (photos, routes, network traffic) are paused'
              : 'Local-only mode: nothing is fetched from the internet'
          }
        >
          {NET_TEXT[net]}
        </span>
      )}
    </button>
  );
}
