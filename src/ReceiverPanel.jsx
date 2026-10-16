import { Sheet, SheetRow, SheetSection } from './Sheet.jsx';
import { AntennaIcon } from './Icons.jsx';
import { batteryState, fmtBattery } from './units.js';

const STATUS_TEXT = {
  searching: 'Not connected',
  connecting: 'Connecting…',
  connected: 'Connected',
  no_traffic: 'Connected, no aircraft in range',
  error: 'Error',
};

export default function ReceiverPanel({ receiver, messages, aircraftCount, onClose }) {
  const stats = receiver.stats ?? {};
  return (
    <Sheet title="Receiver" icon={<AntennaIcon />} onClose={onClose}>
      {receiver.detail && <div className="sheet-error">{receiver.detail}</div>}
      {receiver.status === 'searching' && (
        <p className="sheet-empty">Plug in the PingView receiver, or switch on a Wi-Fi receiver on the same network (or join its PingView-xxxx network). It's detected automatically.</p>
      )}
      <SheetSection>
        <SheetRow label="Status">
          <span className="sheet-value">{STATUS_TEXT[receiver.status] ?? receiver.status}</span>
        </SheetRow>
        <SheetRow label="Port">
          <span className="sheet-value mono">{receiver.port ?? '—'}</span>
        </SheetRow>
        {receiver.battery && (
          <SheetRow label="Battery">
            <span className={`sheet-value battery-${batteryState(receiver.battery)}`}>
              {fmtBattery(receiver.battery)}
            </span>
          </SheetRow>
        )}
        <SheetRow label="Aircraft">
          <span className="sheet-value">{aircraftCount}</span>
        </SheetRow>
        <SheetRow label="Traffic reports">
          <span className="sheet-value">{receiver.msg_rate.toFixed(1)} /s</span>
        </SheetRow>
      </SheetSection>
      <SheetSection title="Diagnostics">
        <SheetRow label="Reports received">
          <span className="sheet-value">{messages}</span>
        </SheetRow>
        <SheetRow label="Frames verified">
          <span className="sheet-value">{stats.verified ?? 0}</span>
        </SheetRow>
        <SheetRow label="Status frames">
          <span className="sheet-value">{stats.other_frames ?? 0}</span>
        </SheetRow>
        <SheetRow label="CRC errors">
          <span className="sheet-value">{stats.crc_errors ?? 0}</span>
        </SheetRow>
      </SheetSection>
    </Sheet>
  );
}
