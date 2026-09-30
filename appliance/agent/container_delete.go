package main

import (
	"context"
	"fmt"
	"strings"
)

func deleteContainerVerified(ctx context.Context, id string, execute func(context.Context, string, ...string) (commandOutput, error)) (commandOutput, error) {
	output, err := execute(ctx, "nerdctl", "--namespace", namespace, "rm", "--force", "--volumes", id)
	if err != nil && !commandReportsNotFound(err, id) {
		return output, err
	}
	// Missing network/volume errors can mention the container ID too. Do not
	// mistake them for a deleted container and remove its only dashboard node.
	remaining, err := execute(ctx, "nerdctl", "--namespace", namespace, "ps", "--all", "--format", "{{.Names}}")
	if err != nil {
		return output, fmt.Errorf("verify container deletion: %w", err)
	}
	for _, name := range strings.Fields(remaining.Stdout) {
		if name == id {
			return output, fmt.Errorf("container %s is still present; deletion did not finish, retry Delete", id)
		}
	}
	removeContainerDisplay(id)
	return output, nil
}
