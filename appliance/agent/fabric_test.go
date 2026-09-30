package main

import "testing"

func TestFabricAddressValidation(t *testing.T) {
	if !validFabricAddress("10.200.12.34", "52:54:4f:c8:0c:22") {
		t.Fatal("valid identity rejected")
	}
	for _, pair := range [][2]string{{"127.0.0.1", "52:54:4f:c8:0c:22"}, {"10.200.12.34", "52:54:4f:c8:0c:23"}, {"10.200.12.255", "52:54:4f:c8:0c:ff"}, {"::1", "52:54:4f:c8:0c:22"}, {"10.224.12.34", "52:54:4f:e0:0c:22"}} {
		if validFabricAddress(pair[0], pair[1]) {
			t.Fatalf("unsafe identity accepted: %v", pair)
		}
	}
}
