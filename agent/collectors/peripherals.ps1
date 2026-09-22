$ErrorActionPreference = 'SilentlyContinue'

function Get-PnpDeviceList($classFilter) {
    Get-CimInstance Win32_PnPEntity | Where-Object { $_.PNPClass -in $classFilter } | ForEach-Object {
        [ordered]@{
            name         = $_.Name
            class        = $_.PNPClass
            manufacturer = $_.Manufacturer
            status       = $_.Status
            pnp_device_id = $_.PNPDeviceID
            service      = $_.Service
            present      = $_.Present
        }
    }
}

$printers = @()
try {
    $defaultPrinter = (Get-Printer -ErrorAction SilentlyContinue | Where-Object Default -eq $true | Select-Object -ExpandProperty Name -First 1)
    $printers = @(Get-Printer -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{
            name      = $_.Name
            driver    = $_.DriverName
            port      = $_.PortName
            shared    = $_.Shared
            published = $_.Published
            default   = ($_.Name -eq $defaultPrinter)
            type      = $_.PrinterStatus
            location  = $_.Location
            comment   = $_.Comment
        }
    })
} catch {}

$usbDevices = @(Get-CimInstance Win32_PnPEntity | Where-Object {
    $_.PNPClass -eq 'USB' -or $_.PNPDeviceID -match '^USB\\'
} | ForEach-Object {
    [ordered]@{
        name = $_.Name; manufacturer = $_.Manufacturer; status = $_.Status
        pnp_device_id = $_.PNPDeviceID; class = $_.PNPClass
    }
})

$bluetooth = @(Get-PnpDeviceList @('Bluetooth','BluetoothLE'))
$cameras   = @(Get-PnpDeviceList @('Image','Camera','Media'))
$storage   = @(Get-PnpDeviceList @('DiskDrive','HDC','SCSIAdapter'))
$display   = @(Get-PnpDeviceList @('Monitor','Display'))
$audio     = @(Get-PnpDeviceList @('MEDIA','AudioEndpoint','Sound'))
$input     = @(Get-PnpDeviceList @('Keyboard','Mouse','HIDClass'))
$gamepad   = @(Get-CimInstance Win32_PnPEntity | Where-Object { $_.Name -match 'gamepad|joystick|controller|xbox|dualshock' } | ForEach-Object {
    [ordered]@{ name = $_.Name; class = $_.PNPClass; manufacturer = $_.Manufacturer; status = $_.Status }
})

$smartCard = @(Get-PnpDeviceList @('SmartCardReader'))
$biometric = @(Get-PnpDeviceList @('Biometric'))

# Portas seriais e paralelas
$ports = @(Get-CimInstance Win32_SerialPort -ErrorAction SilentlyContinue | ForEach-Object {
    [ordered]@{ name = $_.Name; device_id = $_.DeviceID; provider = $_.ProviderType }
})

$result = [ordered]@{
    printers         = $printers
    usb_devices      = $usbDevices
    bluetooth        = @($bluetooth)
    cameras_scanners = @($cameras)
    storage_controllers = @($storage)
    displays         = @($display)
    audio_devices    = @($audio)
    input_devices    = @($input)
    game_controllers = @($gamepad)
    smartcard        = @($smartCard)
    biometric        = @($biometric)
    serial_ports     = @($ports)
    device_counts    = [ordered]@{
        printers    = $printers.Count
        usb         = $usbDevices.Count
        bluetooth   = @($bluetooth).Count
        cameras     = @($cameras).Count
        input       = @($input).Count
        audio       = @($audio).Count
    }
}

$result | ConvertTo-Json -Compress -Depth 8
