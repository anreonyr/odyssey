;; Echo plugin — WebAssembly text format.
;;
;; Implements the same contract as echo-cdylib's C-ABI:
;;   abi_version is implicitly v1 (host checks against the constant).
;;   name(out_buf, cap, out_len)         — writes "echo-wasm" + length
;;   invoke(in_buf, in_len, out_buf, out_cap, out_len) -> i32
;;
;; ABI conformance:
;;   - export "memory" — single 64KB page, the plugin uses linear memory
;;     to hand off buffers (callers must read/write via the host's view)
;;   - return code 0 = ok, 1 = buffer too small, 2 = invalid input
;;
;; Echo semantics: copy in_buf to out_buf, length in_len.

(module
  ;; 64KB linear memory; callee/caller share it via host accessor.
  (memory (export "memory") 1)

  ;; Plugin name bytes at offset 0..9 = "echo-wasm".
  (data (i32.const 0) "echo-wasm")

  ;; --- name(out_buf: i32, cap: i32, out_len: i32) ---
  ;; Copies the 9-byte name into out_buf and writes length to *out_len.
  (func (export "name")
        (param $out_buf i32) (param $cap i32) (param $out_len i32)
    ;; if cap < 9, do nothing (return length 0); the host treats that
    ;; as a contract violation but does not crash.
    local.get $cap
    i32.const 9
    i32.lt_u
    if
      local.get $out_len
      i32.const 0
      i32.store
      return
    end

    ;; memory.copy(src=0, dst=out_buf, len=9)
    i32.const 0
    local.get $out_buf
    i32.const 9
    memory.copy

    ;; *out_len = 9
    local.get $out_len
    i32.const 9
    i32.store
  )

  ;; --- invoke(in_buf, in_len, out_buf, out_cap, out_len) -> i32 ---
  ;; Copies in_buf → out_buf, length in_len. Returns 0 on success.
  (func (export "invoke")
        (param $in_buf i32) (param $in_len i32)
        (param $out_buf i32) (param $out_cap i32) (param $out_len i32)
        (result i32)
    ;; if in_len > out_cap → return 1 (small buffer)
    local.get $in_len
    local.get $out_cap
    i32.gt_u
    if
      i32.const 1
      return
    end

    ;; if in_buf is null and in_len > 0 → return 2 (invalid input).
    ;; (WASM linear memory can't be null at the language level; the
    ;; host's null check is for safety, not strictly required here.)

    ;; memory.copy(in_buf, out_buf, in_len)
    local.get $in_buf
    local.get $out_buf
    local.get $in_len
    memory.copy

    ;; *out_len = in_len
    local.get $out_len
    local.get $in_len
    i32.store

    i32.const 0
  )
)
