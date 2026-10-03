package main

import "time"

// The appliance is x86-64 even on ARM hosts. Starting nerdctl/runc under
// software emulation can exceed the native budget before the service starts.
// Keep Stop bounded independently; no guest request can choose this policy.
func (s *server) lifecycleTimeout(action string) time.Duration {
	if s.emulated && (action == "start" || action == "restart" || action == "resume") {
		return 10 * time.Minute
	}
	return 90 * time.Second
}

func (s *server) diagnosticTimeout() time.Duration {
	if s.emulated {
		return 2 * time.Minute
	}
	return 15 * time.Second
}
