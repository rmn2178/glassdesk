const MANIFEST: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0" xmlns:asmv3="urn:schemas-microsoft-com:asm.v3">
  <assemblyIdentity
    type="win32"
    name="Nosih.Dock"
    version="1.0.0.0"
    processorArchitecture="*"
  />
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <!-- Windows 10 and Windows 11 -->
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
  <asmv3:application>
    <asmv3:windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">PerMonitorV2, PerMonitor</dpiAwareness>
      <longPathAware xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">true</longPathAware>
    </asmv3:windowsSettings>
  </asmv3:application>
</assembly>
"#;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/brand/nosih.ico");
    println!("cargo:rerun-if-changed=build.rs");
    let mut resources = winresource::WindowsResource::new();
    resources.set_icon("../../assets/brand/nosih.ico");
    // Code signing checks that every signed file names the same product.
    resources.set("ProductName", "Nosih");
    resources.set("FileDescription", "Nosih dock");
    // Ensure Microsoft.Windows.Common-Controls 6.0.0.0 is declared so shell
    // extensions (like utshellext.dll) find modern exports like LoadIconWithScaleDown
    // (ordinal 381) instead of failing on comctl32 v5 (issue #23).
    resources.set_manifest(MANIFEST);
    if let Err(error) = resources.compile() {
        // The icon and manifest are cosmetic during non-Windows builds or missing rc:
        // a missing resource compiler must not block the build.
        println!("cargo:warning=no se pudieron incrustar los recursos: {error}");
    }
}
