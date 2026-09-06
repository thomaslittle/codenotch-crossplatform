# Usage reset alerts — follow-up feature

This behavior is intentionally separate from the notch themes PR.

Planned scope:
- detect a real quota-window rollover by comparing consecutive provider snapshots
- require both a reset-time advance and a meaningful used-fraction drop to avoid false positives from stale/corrected data
- persist unread reset events across refreshes/restarts
- mark the specific provider when the notch is visible
- show a compact edge/sliver indicator that remains visible while auto-hide is retracted
- clear an alert when the user opens the affected provider/usage detail
- consider opt-in sound only after the visual detector is proven on real provider resets
