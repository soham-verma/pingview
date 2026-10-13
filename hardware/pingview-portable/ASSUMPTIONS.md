# PingView portable receiver - design assumptions, revision A

Status: engineering prototype design; no fabricated hardware or certification tests yet.

1. Ground-use ADS-B reception only; no aircraft installation or safety-of-flight claims.
2. PingRX Pro supply is 5.0 V, 150 mA nominal, with a 300 mA continuous design load. Its ambiguous 3.3-5 V connector table does not change this choice.
3. The supplied 6-wire GH cable is pin-for-pin. Verify continuity and pin 1 orientation before connecting; wire colours are not sufficient.
4. UART is 3.3 V CMOS, 57600 baud, 8N1, no handshake. Both directions are available. Payloads are forwarded unchanged; MAVLink framing is firmware-only.
5. USB is a 5 V sink and USB 2.0 full-speed peripheral. No PD voltage negotiation, USB host, or battery-to-USB power-bank output.
6. Unknown/unconfigured standard ports get a <=100 mA startup budget. A configured host may grant up to 500 mA; charging and RF use must follow source authorization.
7. Firmware starts with Wi-Fi and Ping off, enumerates before enabling the Ping on a standard host, and handles USB suspend/resume and cable changes.
8. Batteryless wired operation is required after successful host configuration on a capable port. Wi-Fi is disabled if batteryless operation would exceed the USB budget. Suspended ports cannot guarantee continuous batteryless reception.
9. The main switch stops ESP32 and Ping; the charger/source-detection circuits remain able to operate from USB. Slow charging while OFF on a standard computer port is acceptable.
10. A protected 1S, 3.7 V nominal / 4.2 V maximum-charge LiPo is mandatory. Recommended capacity is 3000 mAh; 2000 mAh is supported. Pack must permit at least 1.5 A charge and 2 A continuous discharge, including connector/wire capability.
11. Pack protection includes overcharge, overdischarge, overcurrent and short-circuit cutoff; it is not a substitute for the charger or thermistor. The specified charge current is approximately 1 A.
12. Battery uses JST-PH 2-pin with explicitly checked polarity. A separate keyed 2-pin thermistor lead allows sourceable protected packs without reworking their protection boards.
13. A 10 kohm, 1% NTC with specified beta is bonded to the cell using insulating thermally conductive tape. A detached sensor cannot be detected electrically and must be prevented mechanically. Open or shorted sensor inhibits charging.
14. Operation is specified at 0-45 C ambient; charging must remain within the pack temperature range 0-40 C. Hardware trips use guard bands inside these endpoints to accommodate tolerances.
15. The 45 C enclosure surface limit at 25 C ambient is a prototype acceptance criterion, not a consequence of the charger's much hotter die thermal limit. Charge current may be reduced after enclosure testing.
16. Plastic enclosure; PCB target 70 x 50 x 1.6 mm; four M2.5 corner fixing holes with nonconductive hardware near RF. Battery below the PCB; Ping on an adjacent bracket. Overall enclosure height <=30 mm.
17. The approximately 60 x 40 x 10 mm battery envelope includes tolerance allowances and a separate region for protection electronics and leads; supplier pack drawings govern final enclosure sizing.
18. USB and slide switch share one short edge. Ping coax exits the opposite short edge. ESP32 antenna faces a long edge in a battery-free, metal-free region; its keep-out overrides component density.
19. Four-layer FR-4, 1.6 mm, nominal outer copper 35 um / inner copper 17.5 um. Fabricator confirms impedance geometry against its actual laminate construction.
20. Surface-mount parts are placed on one side where possible. Through-hole mechanical connectors may require a separate assembly operation.
21. ESP32-S3-WROOM-1-N8R2 is the primary module. No strapping pins are assigned to UART, indicators or power controls.
22. Runtime estimates use 100 mA average at 3.3 V for the ESP32 with Wi-Fi active, 150 mA at 5 V for Ping, converter efficiency estimates and a usable-energy allowance. Peaks size supplies, not average runtime.
23. BLE is unimplemented but the module supports a future firmware feature. USB/Wi-Fi forwarding and command arbitration remain firmware responsibilities.
24. Battery voltage is for monitoring; a fuel gauge is optional future work, not necessary for the first revision.
25. Cost estimates are AUD planning figures, using an explicit conversion allowance, not currency forecasts or binding assembly quotes. The A$60 target applies to the assembled carrier PCB at 50 units.
26. Battery, Ping, antenna, enclosure, freight, tax, programming/test fixture and certification are separate from carrier-board assembly cost.
27. LCSC listings establish catalogue identity, not guaranteed JLCPCB assembly stock or basic-part status. Stock, price and substitutions must be rechecked at procurement. No unreviewed substitute is authorized.
28. This delivery contains the full schematic project, netlist, BOM and layout guidance. A routed, fabrication-released PCB is a subsequent layout/verification step; no Gerbers are represented as ready for manufacture.
29. Electronic checks and datasheet calculations cannot establish RF sensitivity, USB compliance, cell authenticity, thermal performance or measured runtime. The bring-up plan defines those acceptance tests.
