import asyncio
import unittest
from vault_gateway import Gateway


class GatewayTest(unittest.IsolatedAsyncioTestCase):
    async def test_binary_stream_is_preserved_in_both_directions(self):
        gateway = Gateway()
        clients = await asyncio.start_server(gateway.client, "127.0.0.1", 0)
        workers = await asyncio.start_server(gateway.worker, "127.0.0.1", 0)
        async with clients, workers:
            upstream_r, upstream_w = await asyncio.open_connection(
                "127.0.0.1", workers.sockets[0].getsockname()[1])
            client_r, client_w = await asyncio.open_connection(
                "127.0.0.1", clients.sockets[0].getsockname()[1])
            ciphertext = bytes(range(256)) * 200
            client_w.write(ciphertext)
            await client_w.drain()
            self.assertEqual(await asyncio.wait_for(upstream_r.readexactly(1), 2), b"\x01")
            self.assertEqual(await asyncio.wait_for(upstream_r.readexactly(len(ciphertext)), 2), ciphertext)
            upstream_w.write(ciphertext[::-1])
            await upstream_w.drain()
            self.assertEqual(await asyncio.wait_for(client_r.readexactly(len(ciphertext)), 2), ciphertext[::-1])
            client_w.close()
            await client_w.wait_closed()
            self.assertEqual(await asyncio.wait_for(upstream_r.read(1), 2), b"")
            upstream_w.close()
            await upstream_w.wait_closed()


if __name__ == "__main__":
    unittest.main()
