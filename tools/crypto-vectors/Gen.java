import io.netty.buffer.ByteBuf;
import io.netty.buffer.Unpooled;
import io.netty.channel.embedded.EmbeddedChannel;
import org.alexdev.havana.game.encryption.Cryptography;
import org.alexdev.havana.game.encryption.DiffieHellman;
import org.alexdev.havana.game.encryption.HugeInt15;
import org.alexdev.havana.game.encryption.SecurityCode;
import org.alexdev.havana.game.player.Player;
import org.alexdev.havana.server.netty.NettyPlayerNetwork;
import org.alexdev.havana.server.netty.codec.EncryptionDecoder;
import org.alexdev.havana.server.netty.codec.EncryptionEncoder;

import java.lang.reflect.Field;
import java.lang.reflect.Method;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

/**
 * Prints test vectors for the Rust port of the encryption layer, computed by the real Havana classes
 * (Cryptography, HugeInt15, SecurityCode, DiffieHellman, NettyPlayerNetwork, EncryptionEncoder and
 * EncryptionDecoder). Output is "name = value" lines; binary values are lowercase hex.
 *
 * The codecs are the unmodified Netty handlers, run inside an EmbeddedChannel. Their Player and
 * NettyPlayerNetwork are allocated without constructors (Unsafe) because the constructors need a
 * running server; only the fields the codecs read are set.
 */
public class Gen {
    static final StringBuilder out = new StringBuilder();

    static void put(String name, Object value) {
        out.append(name).append(" = ").append(value).append('\n');
    }

    static String hex(byte[] b) {
        StringBuilder sb = new StringBuilder();
        for (byte x : b) sb.append(String.format("%02x", x & 0xff));
        return sb.toString();
    }

    static String hex(String latin1) {
        return hex(latin1.getBytes(StandardCharsets.ISO_8859_1));
    }

    static String hex(int[] a) {
        StringBuilder sb = new StringBuilder();
        for (int x : a) sb.append(String.format("%02x", x));
        return sb.toString();
    }

    static void set(Object o, String field, Object v) throws Exception {
        Field f = o.getClass().getDeclaredField(field);
        f.setAccessible(true);
        f.set(o, v);
    }

    static sun.misc.Unsafe unsafe() throws Exception {
        Field f = sun.misc.Unsafe.class.getDeclaredField("theUnsafe");
        f.setAccessible(true);
        return (sun.misc.Unsafe) f.get(null);
    }

    static NettyPlayerNetwork networkWithToken(String token) throws Exception {
        NettyPlayerNetwork net = (NettyPlayerNetwork) unsafe().allocateInstance(NettyPlayerNetwork.class);
        net.setToken(token);
        return net;
    }

    /** Gives the channel a Player whose network was seeded by NettyPlayerNetwork.setToken. */
    static Player playerWithToken(EmbeddedChannel ch, String token) throws Exception {
        Player player = (Player) unsafe().allocateInstance(Player.class);
        set(player, "network", networkWithToken(token));
        ch.attr(Player.PLAYER_KEY).set(player);
        return player;
    }

    static byte[] drain(ByteBuf b) {
        byte[] r = new byte[b.readableBytes()];
        b.readBytes(r);
        return r;
    }

    static String b64(int v, int n) {
        StringBuilder sb = new StringBuilder();
        for (int i = n - 1; i >= 0; i--) sb.append((char) (64 + ((v >> (6 * i)) & 63)));
        return sb.toString();
    }

    static byte[] latin1(String s) {
        return s.getBytes(StandardCharsets.ISO_8859_1);
    }

    public static void main(String[] args) throws Exception {
        // ---- Parameters -------------------------------------------------------------------
        var g15 = new HugeInt15();
        var p15 = new HugeInt15();
        g15.assign(SecurityCode.getLoginParameter("g"), null, true);
        p15.assign(SecurityCode.getLoginParameter("p"), null, true);
        put("dh.g", new BigInteger(g15.getString()));
        put("dh.p", new BigInteger(p15.getString()));
        for (int x : new int[]{0, 1, 5678, 7639, 12345, 32767}) {
            put("securitycode.decode." + x, SecurityCode.decode(x));
        }

        // ---- Diffie-Hellman with fixed private keys ----------------------------------------
        // DiffieHellman picks a random private key; the field is overwritten with a fixed one and
        // the class's own computePublicKey/generateSharedKey do the rest.
        BigInteger[] privates = {
                new BigInteger("9876543210123456789"),
                new BigInteger("1311768467463790320"),
                new BigInteger("18446744073709551557"),
                new BigInteger("42"),
        };
        Method computePublic = DiffieHellman.class.getDeclaredMethod("computePublicKey", BigInteger.class);
        computePublic.setAccessible(true);
        for (int i = 0; i < privates.length; i++) {
            for (int j = 0; j < privates.length; j++) {
                if (i == j) continue;
                var server = new DiffieHellman();
                set(server, "privateKey", privates[i]);
                BigInteger serverPub = (BigInteger) computePublic.invoke(server, privates[i]);
                var client = new DiffieHellman();
                set(client, "privateKey", privates[j]);
                BigInteger clientPub = (BigInteger) computePublic.invoke(client, privates[j]);
                // Server side, exactly as GENERATEKEY does it: the public key arrives as a decimal string.
                server.generateSharedKey(clientPub.toString());
                client.generateSharedKey(serverPub.toString());
                if (!server.getSharedKey().equals(client.getSharedKey())) throw new IllegalStateException("DH mismatch");
                String n = "dh." + i + "." + j;
                put(n + ".server_private", privates[i]);
                put(n + ".server_public", serverPub);
                put(n + ".client_private", privates[j]);
                put(n + ".client_public", clientPub);
                put(n + ".shared", server.getSharedKey());
                put(n + ".key", hex(HugeInt15.getByteArray(server.getSharedKey())));
            }
        }
        // getByteArray: handling of the sign byte that BigInteger.toByteArray adds.
        for (String s : new String[]{"1", "127", "128", "255", "256", "32768", "65535", "16777216"}) {
            put("keybytes." + s, hex(HugeInt15.getByteArray(new BigInteger(s))));
        }

        // ---- Cryptography ------------------------------------------------------------------
        int[][] keys = {
                HugeInt15.getByteArray(new BigInteger("123456789123456789123456789")),
                new int[]{1},
                new int[]{0xff, 0x00, 0x80, 0x7f, 0x01},
                HugeInt15.getByteArray(new BigInteger("340282366920938463463374607431768211455")),
        };
        String[] plains = {
                "A",
                "AB",
                "ABC",
                "ABCD",
                "hello world\u0001",
                "\u0000\u0001\u0002þÿ\u0080\u0081",
                "@@\u0001",
                "The quick brown fox jumps over the lazy dog 0123456789 éèü\u0095",
        };
        for (int k = 0; k < keys.length; k++) {
            put("cipher." + k + ".key", hex(keys[k]));
            Cryptography enc = new Cryptography(keys[k]);
            Cryptography dec = new Cryptography(keys[k]);
            for (int m = 0; m < plains.length; m++) {
                // Same instance for all messages: the keystream carries on between calls.
                String c = enc.AkwGx8bHG2kc1xGG4xbdHPCV0fqvK(plains[m]);
                put("cipher." + k + "." + m + ".plain", hex(plains[m]));
                put("cipher." + k + "." + m + ".encoded", c);
                String back = dec.kg4R6Jo5xjlqtFGs1klMrK4ZTzb3R(c);
                if (!back.equals(plains[m])) throw new IllegalStateException("cipher round trip failed");
            }
        }
        // kg4R on text that no encoder produced, with every length class (mod 4 = 0, 2, 3).
        String table = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        for (int len : new int[]{2, 3, 4, 6, 7, 8, 10, 11, 12, 64}) {
            String s = table.substring(0, len);
            Cryptography dec = new Cryptography(keys[0]);
            put("decode." + len + ".input", s);
            put("decode." + len + ".output", hex(dec.kg4R6Jo5xjlqtFGs1klMrK4ZTzb3R(s)));
        }

        // ---- Token seeds, counter step, padding -------------------------------------------
        String[] tokens = {"123456789012345678901234", "000000000000000000000000", "999999999999999999999999",
                "391245501337402612345678", "105040302010987654321098"};
        for (int t = 0; t < tokens.length; t++) {
            NettyPlayerNetwork net = networkWithToken(tokens[t]);
            put("token." + t, tokens[t]);
            put("token." + t + ".tx", net.getTx());
            put("token." + t + ".rx", net.getRx());
        }
        StringBuilder seq = new StringBuilder();
        int s = 0x1234;
        for (int i = 0; i < 12; i++) {
            s = NettyPlayerNetwork.iterateRandom(s);
            seq.append(s).append(' ');
        }
        put("iterate.from_4660", seq.toString().trim());
        put("iterate.65535", NettyPlayerNetwork.iterateRandom(65535));
        for (int[] c : new int[][]{{0, 0}, {3, 0}, {3, 2}, {3, 3}, {3, 4}, {1, 5}, {10, 4}}) {
            String body = "abcdefghij".substring(0, c[0]);
            put("removepad." + c[0] + "." + c[1], hex(NettyPlayerNetwork.removePadding(body, c[1])));
        }
        // addPad is random (SecureRandom): only its shape is checkable.
        for (int n = 0; n < 5; n++) {
            String padded = NettyPlayerNetwork.addPad("xyz", n);
            put("addpad." + n + ".length", padded.length());
            put("addpad." + n + ".suffix", hex(padded.substring(n)));
            boolean ok = true;
            for (int i = 0; i < n; i++) ok &= padded.charAt(i) >= 1 && padded.charAt(i) <= 255;
            put("addpad." + n + ".bytes_in_1_255", ok);
        }

        // ---- Codecs ---------------------------------------------------------------------------
        // Messages: server frames are "B64(2) header + body + 0x01"; client payloads are
        // "B64(2) header + body" (the decoder puts the B64(3) length back in front).
        List<String> serverFrames = new ArrayList<>();
        List<String> clientPayloads = new ArrayList<>();
        String[] bodies = {"", "x", "hello", "a longer body with é and \u0095 inside", "z".repeat(70), "q".repeat(130),
                "\u0002\u0003ÿ\u0001", "w".repeat(4100), "m".repeat(20), "abc", "defg", "hijklm"};
        for (int i = 0; i < bodies.length; i++) {
            serverFrames.add(b64(i + 1, 2) + bodies[i] + "\u0001");
            clientPayloads.add(b64(2002 - i, 2) + bodies[i]);
        }
        String sharedKeyDec = "123456789123456789123456789123456789";
        BigInteger sharedKey = new BigInteger(sharedKeyDec);
        put("codec.key", hex(HugeInt15.getByteArray(sharedKey)));
        for (int t = 0; t < 2; t++) {
            String token = tokens[t == 0 ? 0 : 3];
            put("codec." + t + ".token", token);

            // Server -> client: the unmodified EncryptionEncoder, counters seeded as on a real connection.
            var encCh = new EmbeddedChannel(new EncryptionEncoder(sharedKey));
            playerWithToken(encCh, token);
            put("codec." + t + ".server.count", serverFrames.size());
            for (int i = 0; i < serverFrames.size(); i++) {
                encCh.writeOutbound(Unpooled.wrappedBuffer(latin1(serverFrames.get(i))));
                byte[] enc = drain(encCh.readOutbound());
                put("codec." + t + ".server." + i + ".plain", hex(serverFrames.get(i)));
                put("codec." + t + ".server." + i + ".encrypted", hex(enc));
            }

            // Client -> server: the same encoder acts as the client (its counter starts at the
            // server's tx seed), then the unmodified EncryptionDecoder decodes it.
            var cliCh = new EmbeddedChannel(new EncryptionEncoder(sharedKey));
            Player cliPlayer = playerWithToken(cliCh, token);
            cliPlayer.getNetwork().setRx(cliPlayer.getNetwork().getTx());
            var decCh = new EmbeddedChannel(new EncryptionDecoder(sharedKey));
            playerWithToken(decCh, token);
            put("codec." + t + ".client.count", clientPayloads.size());
            for (int i = 0; i < clientPayloads.size(); i++) {
                cliCh.writeOutbound(Unpooled.wrappedBuffer(latin1(clientPayloads.get(i))));
                byte[] enc = drain(cliCh.readOutbound());
                decCh.writeInbound(Unpooled.wrappedBuffer(enc));
                byte[] dec = drain(decCh.readInbound());
                put("codec." + t + ".client." + i + ".payload", hex(clientPayloads.get(i)));
                put("codec." + t + ".client." + i + ".encrypted", hex(enc));
                put("codec." + t + ".client." + i + ".decoded", hex(dec));
            }
        }

        System.out.print(out);
    }
}
