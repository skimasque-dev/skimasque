Upstream: netstack-smoltcp 0.2.4, MIT OR Apache-2.0. Original crate SHA256: 4c38f66cdd673ff0e760752f27c6d34a7e3a140f0b1eea9efae3c46d8867c83d.

Local patches: bounded 512-item TCP internal channels; deduplicated full-tuple SYN admission before allocation, with 1024 socket permits; release on abort/drop; half-close completion after FIN scheduling; invalid UDP continues, empty UDP preserved; sound Send bound instead of unsafe blanket Send. Original license files retained. smoltcp remains the existing registry TCP/IP implementation.
