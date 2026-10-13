import { ALTITUDE_STOPS, ON_GROUND_COLOR } from './planeIcon.js';
import { legendLabels, useUnits } from './units.js';

const gradientCss = `linear-gradient(to right, ${ALTITUDE_STOPS.map(([, color]) => color).join(', ')})`;

export default function AltitudeLegend() {
  const units = useUnits();
  return (
    <div className="altitude-legend">
      <div>
        <span className="ground-swatch" style={{ background: ON_GROUND_COLOR }} />
        ground
      </div>
      <div className="bar" style={{ background: gradientCss }} />
      <div className="labels">
        {legendLabels(units).map((label) => (
          <span key={label}>{label}</span>
        ))}
      </div>
    </div>
  );
}
