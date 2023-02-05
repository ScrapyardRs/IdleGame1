package rs.scrapyard.minehut.injector;

import com.velocitypowered.api.plugin.Plugin;

import java.io.File;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.attribute.PosixFilePermission;
import java.util.HashSet;
import java.util.Set;
import java.util.logging.LogManager;

@Plugin(
    id = "minehut_injector",
    description = "Minehut injector for running rust servers in minehut.",
    authors = {
        "Corey Shupe (FiXed)"
    },
    name = "Minehut Injector",
    url = "https://github.com/ScrapyardRs/IdleGame1",
    version = "0.0.1"
)
public class MinehutInjectorPlugin {
    public MinehutInjectorPlugin() throws IOException {
        System.out.println("Done (0.67s)!");
        //noinspection InfiniteLoopStatement
        while (true) {
            File file = new File("/home/minecraft/server/server");

            Set<PosixFilePermission> perms = new HashSet<>();
            perms.add(PosixFilePermission.OWNER_READ);
            perms.add(PosixFilePermission.OWNER_WRITE);
            perms.add(PosixFilePermission.GROUP_READ);
            perms.add(PosixFilePermission.GROUP_WRITE);
            perms.add(PosixFilePermission.OTHERS_READ);
            perms.add(PosixFilePermission.OTHERS_WRITE);
            perms.add(PosixFilePermission.GROUP_EXECUTE);
            perms.add(PosixFilePermission.OWNER_EXECUTE);
            perms.add(PosixFilePermission.OTHERS_EXECUTE);
            
            Files.setPosixFilePermissions(file.toPath(), perms);

            ProcessBuilder pb = new ProcessBuilder("/home/minecraft/server/server");
            pb.directory(new File("/home/minecraft/server"));
            pb.inheritIO();
            try {
                pb.start().waitFor();
            } catch (InterruptedException | IOException e) {
                LogManager.getLogManager()
                    .getLogger("MinehutInjectorPlugin")
                    .severe("Exception in server runtime");
                e.printStackTrace();
                System.exit(0);
            }
        }
    }
}
