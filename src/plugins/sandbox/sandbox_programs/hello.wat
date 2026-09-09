;; Hello-world WASM program for the sandbox.
;;
;; Runs three host.log() calls, then a busy loop of 5000 iterations
;; to consume fuel. With fuel=5 the loop traps before completion,
;; demonstrating the resource limit.
;;
;; Memory layout:
;;   offset 0..24   "=== sandbox program ===\n"
;;   offset 24..38  "Hello, world!\n"
;;   offset 38..43  "done\n"

(module
  (import "host" "log" (func $log (param i32 i32)))
  (memory (export "memory") 1)

  (data (i32.const 0)  "=== sandbox program ===\n")
  (data (i32.const 24) "Hello, world!\n")
  (data (i32.const 38) "done\n")

  (func (export "run") (result i32)
    (local $i i32)
    (local $acc i32)
    ;; log(0, 24) — header
    i32.const 0
    i32.const 24
    call $log

    ;; log(24, 14) — hello
    i32.const 24
    i32.const 14
    call $log

    ;; Busy loop: 5000 iterations of "do something". Each iteration
    ;; costs several wasm instructions of fuel. With fuel=5 this
    ;; will trap mid-loop; with fuel=1_000_000 it completes.
    i32.const 0
    local.set $i
    block $done
      loop $again
        ;; if i >= 5000, break
        local.get $i
        i32.const 5000
        i32.ge_s
        br_if $done
        ;; acc = acc + i
        local.get $acc
        local.get $i
        i32.add
        local.set $acc
        ;; i = i + 1
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $again
      end
    end

    ;; log(38, 5) — done
    i32.const 38
    i32.const 5
    call $log

    i32.const 0   ;; exit 0
  )
)
