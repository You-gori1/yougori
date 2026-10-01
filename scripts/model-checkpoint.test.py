"""Real safetensors loading/generation with tiny random models; no GPU or weight download.

Run in an isolated Python environment with the model_server.py dependency versions
and PyTorch 2.8.0. CPU PyTorch is sufficient for these checkpoint regressions.
"""
import importlib.util
import tempfile
import unittest


@unittest.skipUnless(importlib.util.find_spec("torch") and importlib.util.find_spec("transformers"),
                     "requires the model runtime's PyTorch and Transformers dependencies")
class CheckpointTests(unittest.TestCase):
    def roundtrip(self, configuration, multimodal_class=None):
        import torch
        from transformers import AutoModelForCausalLM

        torch.set_num_threads(1)
        torch.manual_seed(47)
        self.assertIn(type(configuration), AutoModelForCausalLM._model_mapping)
        original = (multimodal_class(configuration) if multimodal_class
                    else AutoModelForCausalLM.from_config(configuration)).to(torch.bfloat16).eval()
        inputs = {"input_ids": torch.tensor([[1, 3, 4]]), "attention_mask": torch.ones(1, 3, dtype=torch.long)}
        with tempfile.TemporaryDirectory() as folder:
            original.save_pretrained(folder, safe_serialization=True)
            loaded = AutoModelForCausalLM.from_pretrained(folder, trust_remote_code=False, use_safetensors=True,
                                                         dtype="auto", device_map={"": "cpu"}).eval()
            self.assertEqual(next(loaded.parameters()).dtype, torch.bfloat16)
            with torch.inference_mode():
                torch.testing.assert_close(loaded(**inputs).logits, original(**inputs).logits, rtol=0, atol=0)
                output = loaded.generate(**inputs, max_new_tokens=2, do_sample=False)
            self.assertGreater(output.shape[-1], inputs["input_ids"].shape[-1])

    def qwen_text(self):
        return dict(vocab_size=64, hidden_size=32, intermediate_size=64, num_hidden_layers=2,
                    num_attention_heads=4, num_key_value_heads=2, head_dim=8,
                    layer_types=["linear_attention", "full_attention"], full_attention_interval=2,
                    linear_num_key_heads=2, linear_num_value_heads=4, linear_key_head_dim=8,
                    linear_value_head_dim=8, max_position_embeddings=2048, pad_token_id=0,
                    bos_token_id=1, eos_token_id=2, dtype="bfloat16")

    def qwen_multimodal(self, config_class):
        return config_class(text_config=self.qwen_text(), dtype="bfloat16", image_token_id=60, video_token_id=61,
                            vision_start_token_id=62, vision_end_token_id=63,
                            vision_config=dict(depth=1, hidden_size=32, intermediate_size=64, num_heads=4,
                                               out_hidden_size=32, patch_size=2, spatial_merge_size=1,
                                               temporal_patch_size=1, num_position_embeddings=16))

    def test_qwen3_5_text_checkpoint_loads_and_generates(self):
        from transformers import Qwen3_5TextConfig
        self.roundtrip(Qwen3_5TextConfig(**self.qwen_text()))

    def test_qwen3_5_multimodal_checkpoint_retains_its_text_weights(self):
        from transformers import Qwen3_5Config, Qwen3_5ForConditionalGeneration
        self.roundtrip(self.qwen_multimodal(Qwen3_5Config), Qwen3_5ForConditionalGeneration)

    def test_qwen3_5_moe_checkpoint_loads_and_generates(self):
        from transformers import Qwen3_5MoeTextConfig
        self.roundtrip(Qwen3_5MoeTextConfig(**self.qwen_text(), num_experts=2, num_experts_per_tok=1,
                                           moe_intermediate_size=64, shared_expert_intermediate_size=64))

    def test_llama_checkpoint_remains_compatible(self):
        from transformers import LlamaConfig
        self.roundtrip(LlamaConfig(vocab_size=64, hidden_size=32, intermediate_size=64, num_hidden_layers=1,
                                  num_attention_heads=4, num_key_value_heads=2, max_position_embeddings=2048,
                                  pad_token_id=0, bos_token_id=1, eos_token_id=2, dtype="bfloat16"))


if __name__ == "__main__":
    unittest.main()
