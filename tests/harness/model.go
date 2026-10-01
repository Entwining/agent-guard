package harness

import (
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"sync"
	"time"
)

type ToolResult struct {
	Text    string          `json:"text"`
	IsError bool            `json:"is_error"`
	Raw     json.RawMessage `json:"raw"`
}

// The runtime and HTTP handler share only the active synthetic call and its result.
type ScriptedModel struct {
	mu               sync.Mutex
	runtime, command string
	result           *ToolResult
	requests         int
	error            string
}

func (model *ScriptedModel) Begin(runtime, command string) {
	model.mu.Lock()
	defer model.mu.Unlock()
	model.runtime, model.command, model.result, model.requests, model.error = runtime, command, nil, 0, ""
}

func (model *ScriptedModel) Result() (*ToolResult, int, string) {
	model.mu.Lock()
	defer model.mu.Unlock()
	return model.result, model.requests, model.error
}

func (model *ScriptedModel) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"data":[]}`))
		return
	}
	model.mu.Lock()
	defer model.mu.Unlock()
	model.requests++
	var request struct {
		Model  string `json:"model"`
		Stream bool   `json:"stream"`
		Input  []struct {
			Type   string          `json:"type"`
			Output json.RawMessage `json:"output"`
		} `json:"input"`
		Messages []struct {
			Content json.RawMessage `json:"content"`
		} `json:"messages"`
	}
	if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 16<<20)).Decode(&request); err != nil {
		model.error = err.Error()
		http.Error(w, "invalid model request", http.StatusBadRequest)
		return
	}
	if model.runtime == "codex" {
		for _, item := range request.Input {
			if item.Type != "function_call_output" {
				continue
			}
			var text string
			if err := json.Unmarshal(item.Output, &text); err != nil {
				model.error = err.Error()
				http.Error(w, "invalid tool output", 400)
				return
			}
			model.result = &ToolResult{Text: text, Raw: item.Output}
		}
		var item map[string]any
		if model.result == nil {
			args, _ := json.Marshal(map[string]any{"cmd": model.command, "yield_time_ms": 1000, "max_output_tokens": 1000})
			item = map[string]any{"type": "function_call", "id": "fc_harness", "call_id": "call_harness", "name": "exec_command", "arguments": string(args), "status": "completed"}
		} else {
			item = map[string]any{"type": "message", "id": "msg_harness", "role": "assistant", "status": "completed", "content": []any{map[string]any{"type": "output_text", "text": "done", "annotations": []any{}}}}
		}
		response := map[string]any{"id": fmt.Sprintf("resp_harness_%d", model.requests), "object": "response", "created_at": time.Now().Unix(), "status": "completed", "model": request.Model, "output": []any{item}, "usage": map[string]any{"input_tokens": 10, "output_tokens": 10, "total_tokens": 20, "input_tokens_details": map[string]int{"cached_tokens": 0}, "output_tokens_details": map[string]int{"reasoning_tokens": 0}}}
		started := map[string]any{}
		for k, v := range response {
			started[k] = v
		}
		started["status"], started["output"] = "in_progress", []any{}
		events := []map[string]any{{"type": "response.created", "response": started}, {"type": "response.output_item.added", "output_index": 0, "item": item}, {"type": "response.output_item.done", "output_index": 0, "item": item}, {"type": "response.completed", "response": response}}
		w.Header().Set("Content-Type", "text/event-stream")
		for i, event := range events {
			event["sequence_number"] = i
			model.event(w, event["type"].(string), event)
		}
		return
	}
	if !strings.Contains(r.URL.Path, "/messages") {
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"input_tokens":10}`))
		return
	}
	if len(request.Messages) > 0 {
		var parts []json.RawMessage
		if json.Unmarshal(request.Messages[len(request.Messages)-1].Content, &parts) == nil {
			for _, part := range parts {
				var item struct {
					Type    string          `json:"type"`
					IsError bool            `json:"is_error"`
					Content json.RawMessage `json:"content"`
				}
				if json.Unmarshal(part, &item) == nil && item.Type == "tool_result" {
					var text string
					if json.Unmarshal(item.Content, &text) != nil {
						var content []struct {
							Text string `json:"text"`
						}
						if err := json.Unmarshal(item.Content, &content); err != nil {
							model.error = err.Error()
							http.Error(w, "invalid tool result", 400)
							return
						}
						for _, block := range content {
							text += block.Text
						}
					}
					model.result = &ToolResult{Text: text, IsError: item.IsError, Raw: part}
				}
			}
		}
	}
	name := "bash"
	if model.runtime == "claude" {
		name = "Bash"
	}
	content := map[string]any{"type": "tool_use", "id": "tool_harness", "name": name, "input": map[string]string{"command": model.command}}
	stop := "tool_use"
	if model.result != nil {
		content = map[string]any{"type": "text", "text": "done"}
		stop = "end_turn"
	}
	message := map[string]any{"id": "msg_harness", "type": "message", "role": "assistant", "model": request.Model, "content": []any{content}, "stop_reason": stop, "stop_sequence": nil, "usage": map[string]int{"input_tokens": 10, "output_tokens": 10}}
	if !request.Stream {
		w.Header().Set("Content-Type", "application/json")
		if err := json.NewEncoder(w).Encode(message); err != nil {
			model.error = err.Error()
		}
		return
	}
	start := map[string]any{}
	for k, v := range message {
		start[k] = v
	}
	start["content"], start["stop_reason"] = []any{}, nil
	block := map[string]any{"type": "text", "text": ""}
	delta := map[string]any{"type": "text_delta", "text": "done"}
	if model.result == nil {
		block = map[string]any{"type": "tool_use", "id": "tool_harness", "name": name, "input": map[string]any{}}
		input, _ := json.Marshal(content["input"])
		delta = map[string]any{"type": "input_json_delta", "partial_json": string(input)}
	}
	w.Header().Set("Content-Type", "text/event-stream")
	for _, event := range []map[string]any{{"type": "message_start", "message": start}, {"type": "content_block_start", "index": 0, "content_block": block}, {"type": "content_block_delta", "index": 0, "delta": delta}, {"type": "content_block_stop", "index": 0}, {"type": "message_delta", "delta": map[string]any{"stop_reason": stop, "stop_sequence": nil}, "usage": map[string]int{"output_tokens": 10}}, {"type": "message_stop"}} {
		model.event(w, event["type"].(string), event)
	}
}

func (model *ScriptedModel) event(w http.ResponseWriter, name string, value any) {
	body, err := json.Marshal(value)
	if err != nil {
		model.error = err.Error()
		return
	}
	if _, err = fmt.Fprintf(w, "event: %s\ndata: %s\n\n", name, body); err != nil {
		model.error = err.Error()
	}
}
