package main

import (
	"encoding/json"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
)

type workloadVolume struct {
	Source   string `json:"source"`
	Target   string `json:"target"`
	ReadOnly bool   `json:"readOnly"`
}
type workloadOptions struct {
	Environment map[string]string `json:"environment"`
	Hosts       map[string]string `json:"hosts"`
	Args        *[]string         `json:"args"`
	Entrypoint  *[]string         `json:"entrypoint"`
	WorkingDir  *string           `json:"workingDir"`
	User        *string           `json:"user"`
	Volumes     []workloadVolume  `json:"volumes"`
	Binds       []workloadVolume  `json:"binds"`
	Restart     string            `json:"restart"`
}

var workloadName = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9_.-]{0,79}$`)
var variableName = regexp.MustCompile(`^[A-Za-z_][A-Za-z0-9_]{0,255}$`)

func workloadPath(s string) bool {
	if !strings.HasPrefix(s, "/") || s == "/" || len(s) > 4096 || strings.ContainsAny(s, "\x00\r\n\\,:") {
		return false
	}
	for _, part := range strings.Split(s, "/") {
		if part == ".." || part == "." {
			return false
		}
	}
	for _, reserved := range []string{"/proc", "/sys", "/dev", "/opendock"} {
		if s == reserved || strings.HasPrefix(s, reserved+"/") {
			return false
		}
	}
	return true
}
func (o workloadOptions) arguments() ([]string, error) {
	args := []string{}
	if len(o.Environment) > 512 || len(o.Volumes) > 64 {
		return nil, fmt.Errorf("too many workload options")
	}
	names := make([]string, 0, len(o.Environment))
	for k, v := range o.Environment {
		if !variableName.MatchString(k) || len(v) > 65536 || strings.ContainsRune(v, 0) {
			return nil, fmt.Errorf("invalid environment variable")
		}
		names = append(names, k)
	}
	sort.Strings(names)
	for _, k := range names {
		args = append(args, "--env", k+"="+o.Environment[k])
	}
	if len(o.Hosts) > 256 {
		return nil, fmt.Errorf("too many host aliases")
	}
	for name, ip := range o.Hosts {
		if !workloadName.MatchString(name) || net.ParseIP(ip).To4() == nil {
			return nil, fmt.Errorf("invalid host alias")
		}
		args = append(args, "--add-host", name+":"+ip)
	}
	if o.WorkingDir != nil {
		if *o.WorkingDir != "/" && !workloadPath(*o.WorkingDir) {
			return nil, fmt.Errorf("invalid working directory")
		}
		args = append(args, "--workdir", *o.WorkingDir)
	}
	if o.User != nil {
		if *o.User == "" || strings.HasPrefix(*o.User, "-") || strings.ContainsAny(*o.User, "\x00\r\n") {
			return nil, fmt.Errorf("invalid user")
		}
		args = append(args, "--user", *o.User)
	}
	if o.Restart != "" {
		switch o.Restart {
		case "no", "always", "unless-stopped", "on-failure":
			args = append(args, "--restart", o.Restart)
		default:
			return nil, fmt.Errorf("invalid restart policy")
		}
	}
	targets := map[string]bool{}
	for _, v := range o.Volumes {
		if !workloadName.MatchString(v.Source) || !workloadPath(v.Target) || targets[v.Target] {
			return nil, fmt.Errorf("invalid volume")
		}
		targets[v.Target] = true
		mount := v.Source + ":" + v.Target
		if v.ReadOnly {
			mount += ":ro"
		}
		args = append(args, "--volume", mount)
	}
	for _, v := range o.Binds {
		if !workloadName.MatchString(v.Source) || !workloadPath(v.Target) || targets[v.Target] {
			return nil, fmt.Errorf("invalid PC volume")
		}
		targets[v.Target] = true
		mount := filepath.Join(dataRoot, "workload-mounts", v.Source) + ":" + v.Target
		if v.ReadOnly {
			mount += ":ro"
		}
		args = append(args, "--volume", mount)
	}
	for _, list := range []*[]string{o.Args, o.Entrypoint} {
		if list != nil {
			if len(*list) > 512 {
				return nil, fmt.Errorf("too many command arguments")
			}
			for _, v := range *list {
				if len(v) > 65536 || strings.ContainsRune(v, 0) {
					return nil, fmt.Errorf("invalid command argument")
				}
			}
		}
	}
	if o.Entrypoint != nil {
		encoded, _ := json.Marshal(*o.Entrypoint)
		args = append(args, "--entrypoint", string(encoded))
	}
	return args, nil
}
func appendWorkload(args []string, image, command string, o workloadOptions) ([]string, error) {
	options, err := o.arguments()
	if err != nil {
		return nil, err
	}
	args = append(args, options...)
	if strings.TrimSpace(command) != "" {
		args = append(args, "--entrypoint", "/bin/sh", image, "-lc", command)
	} else {
		args = append(args, image)
		if o.Args != nil {
			args = append(args, (*o.Args)...)
		}
	}
	return args, nil
}
func saveWorkload(id string, o workloadOptions) error {
	root := filepath.Join(dataRoot, "workload-options")
	if err := os.MkdirAll(root, 0700); err != nil {
		return err
	}
	data, err := json.Marshal(o)
	if err != nil {
		return err
	}
	temp, err := os.CreateTemp(root, ".options-")
	if err != nil {
		return err
	}
	name := temp.Name()
	defer os.Remove(name)
	if _, err = temp.Write(data); err == nil {
		err = temp.Sync()
	}
	closeErr := temp.Close()
	if err != nil {
		return err
	}
	if closeErr != nil {
		return closeErr
	}
	return os.Rename(name, filepath.Join(root, id+".json"))
}
func readWorkload(id string) (workloadOptions, error) {
	var o workloadOptions
	data, err := os.ReadFile(filepath.Join(dataRoot, "workload-options", id+".json"))
	if os.IsNotExist(err) {
		return o, nil
	}
	if err != nil {
		return o, err
	}
	if len(data) > 256*1024 {
		return o, fmt.Errorf("invalid workload metadata")
	}
	err = json.Unmarshal(data, &o)
	return o, err
}
