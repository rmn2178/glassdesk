// Build-runner check only. Uses Windows Attachment Services without opening a file.
using System;
using System.Runtime.InteropServices;

namespace NosihRelease
{
    [ComImport, Guid("73DB1241-1E85-4581-8E4F-A81E1D0F8C57")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    internal interface IAttachmentExecute
    {
        [PreserveSig] int SetClientTitle([MarshalAs(UnmanagedType.LPWStr)] string title);
        [PreserveSig] int SetClientGuid(ref Guid guid);
        [PreserveSig] int SetLocalPath([MarshalAs(UnmanagedType.LPWStr)] string path);
        [PreserveSig] int SetFileName([MarshalAs(UnmanagedType.LPWStr)] string name);
        [PreserveSig] int SetSource([MarshalAs(UnmanagedType.LPWStr)] string source);
        [PreserveSig] int SetReferrer([MarshalAs(UnmanagedType.LPWStr)] string referrer);
        [PreserveSig] int CheckPolicy();
        [PreserveSig] int Prompt(IntPtr parent, uint prompt, out uint action);
        [PreserveSig] int Save();
    }

    public static class AttachmentCheck
    {
        private static void RequireSuccess(int result)
        {
            // Even an unexpected positive status must not inherit a clean verdict.
            if (result != 0)
                throw new COMException(String.Format(
                    "Windows Attachment Services refused or could not complete the check (HRESULT 0x{0:X8}).", result), result);
        }

        public static void Check(string file, string source)
        {
            var type = Type.GetTypeFromCLSID(new Guid("4125DD96-E03A-4103-8F70-E0597D803B9C"), true);
            object instance = Activator.CreateInstance(type);
            try
            {
                var attachment = (IAttachmentExecute)instance;
                // Identifies this verifier, never impersonates a browser or a trusted publisher.
                var client = new Guid("A95CC074-416D-49EC-BD51-42A4C825569B");
                RequireSuccess(attachment.SetClientGuid(ref client));
                RequireSuccess(attachment.SetLocalPath(file));
                RequireSuccess(attachment.SetSource(source));
                // Save may call the registered antivirus and remove or modify a rejected file.
                // No Execute, Prompt or SaveWithUI method is ever invoked.
                RequireSuccess(attachment.Save());
            }
            finally
            {
                Marshal.FinalReleaseComObject(instance);
            }
        }
    }
}
