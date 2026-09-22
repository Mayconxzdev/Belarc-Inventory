$ErrorActionPreference = 'SilentlyContinue'

function Convert-WmiByteString($bytes) {
    if (-not $bytes) { return $null }
    -join ($bytes | Where-Object { $_ -gt 0 } | ForEach-Object { [char]$_ })
}

function Get-ChassisTypeName($code) {
    $map = @{
        1='Other'; 2='Unknown'; 3='Desktop'; 4='Low Profile Desktop'; 5='Pizza Box'
        6='Mini Tower'; 7='Tower'; 8='Portable'; 9='Laptop'; 10='Notebook'
        11='Hand Held'; 12='Docking Station'; 13='All in One'; 14='Sub Notebook'
        15='Space-Saving'; 16='Lunch Box'; 17='Main Server Chassis'; 18='Expansion Chassis'
        23='Rack Mount'; 30='Tablet'; 31='Convertible'; 32='Detachable'
    }
    if ($map.ContainsKey([int]$code)) { $map[[int]$code] } else { "Type $code" }
}

$cs   = Get-CimInstance Win32_ComputerSystem
$prod = Get-CimInstance Win32_ComputerSystemProduct
$bios = Get-CimInstance Win32_BIOS
$board = Get-CimInstance Win32_BaseBoard
$enclosure = Get-CimInstance Win32_SystemEnclosure | Select-Object -First 1

$cpus = @(Get-CimInstance Win32_Processor | ForEach-Object {
    [ordered]@{
        name                 = ($_.Name -replace '\s+', ' ').Trim()
        manufacturer         = $_.Manufacturer
        architecture         = $_.Architecture
        family               = $_.Family
        processor_id         = $_.ProcessorId
        socket               = $_.SocketDesignation
        cores                = $_.NumberOfCores
        logical_processors   = $_.NumberOfLogicalProcessors
        max_clock_mhz        = $_.MaxClockSpeed
        current_clock_mhz    = $_.CurrentClockSpeed
        l2_cache_kb          = $_.L2CacheSize
        l3_cache_kb          = $_.L3CacheSize
        stepping             = $_.Stepping
        revision             = $_.Revision
        voltage              = $_.CurrentVoltage
        status               = $_.Status
        load_percent         = $_.LoadPercentage
    }
})

$memModules = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object {
    [ordered]@{
        bank_label    = $_.BankLabel
        locator       = $_.DeviceLocator
        capacity_gb   = [math]::Round($_.Capacity / 1GB, 2)
        speed_mhz     = $_.Speed
        configured_mhz = $_.ConfiguredClockSpeed
        manufacturer  = ($_.Manufacturer -replace '\s+$', '').Trim()
        part_number   = ($_.PartNumber -replace '\s+$', '').Trim()
        serial        = ($_.SerialNumber -replace '\s+$', '').Trim()
        form_factor   = $_.FormFactor
        memory_type   = $_.MemoryType
        data_width    = $_.DataWidth
        total_width   = $_.TotalWidth
    }
})
$ramTotal = 0.0
foreach ($m in $memModules) {
    if ($null -ne $m.capacity_gb) { $ramTotal += [double]$m.capacity_gb }
}
if ($ramTotal -le 0 -and $cs.TotalPhysicalMemory -gt 0) {
    $ramTotal = [math]::Round($cs.TotalPhysicalMemory / 1GB, 2)
}
$ramTotal = [math]::Round($ramTotal, 2)

$logicalDisks = @(Get-CimInstance Win32_LogicalDisk | Where-Object { $_.DriveType -eq 3 } | ForEach-Object {
    $freePct = if ($_.Size -gt 0) { [math]::Round(($_.FreeSpace / $_.Size) * 100, 1) } else { 0 }
    [ordered]@{
        letter = $_.DeviceID; label = $_.VolumeName; filesystem = $_.FileSystem
        size_gb = [math]::Round($_.Size / 1GB, 2); free_gb = [math]::Round($_.FreeSpace / 1GB, 2)
        free_percent = $freePct; volume_serial = $_.VolumeSerialNumber
    }
})

$physicalDisks = @(Get-CimInstance Win32_DiskDrive | ForEach-Object {
    $pd = $null
    $reliability = $null
    try {
        $pd = Get-PhysicalDisk -DeviceId $_.Index -ErrorAction SilentlyContinue
        if ($pd) {
            $reliability = Get-StorageReliabilityCounter -PhysicalDisk $pd -ErrorAction SilentlyContinue
        }
    } catch {}
    $relObj = $null
    if ($reliability) {
        $relObj = [ordered]@{
            temperature_celsius     = if ($reliability.Temperature) { $reliability.Temperature } else { $null }
            wear_percent            = if ($null -ne $reliability.Wear) { $reliability.Wear } else { $null }
            read_errors_total       = $reliability.ReadErrorsTotal
            write_errors_total      = $reliability.WriteErrorsTotal
            read_errors_uncorrected = $reliability.ReadErrorsUncorrected
            write_errors_uncorrected = $reliability.WriteErrorsUncorrected
            flush_latency_max_ms    = $reliability.FlushLatencyMax
            load_unload_cycle_count = $reliability.LoadUnloadCycleCount
            power_on_hours          = $reliability.PowerOnHours
        }
    }
    [ordered]@{
        model = $_.Model; serial = ($_.SerialNumber -replace '\s+$', '').Trim()
        interface = $_.InterfaceType; media_type = if ($pd) { $pd.MediaType } else { $_.MediaType }
        size_gb = [math]::Round($_.Size / 1GB, 2); partitions = $_.Partitions
        status = $_.Status; firmware = $_.FirmwareRevision; scsi_bus = $_.SCSIBus
        health_status = if ($pd) { $pd.HealthStatus } else { $null }
        operational_status = if ($pd) { $pd.OperationalStatus } else { $null }
        reliability = $relObj
    }
})

$gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
    [ordered]@{
        name = $_.Name; adapter_ram_mb = if ($_.AdapterRAM -and $_.AdapterRAM -gt 0) { [math]::Round($_.AdapterRAM / 1MB, 0) } else { $null }
        driver_version = $_.DriverVersion; driver_date = $_.DriverDate
        video_processor = $_.VideoProcessor; pnp_device_id = $_.PNPDeviceID
        current_resolution = if ($_.CurrentHorizontalResolution) { "$($_.CurrentHorizontalResolution)x$($_.CurrentVerticalResolution)" } else { $_.VideoModeDescription }
        refresh_rate = $_.CurrentRefreshRate; bits_per_pixel = $_.CurrentBitsPerPixel
        status = $_.Status
    }
})

# Monitores via WMI (EDID)
$monitors = @()
try {
    $monitorIds = Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorID -ErrorAction SilentlyContinue
    foreach ($m in $monitorIds) {
        $name = Convert-WmiByteString($m.UserFriendlyName)
        $serial = Convert-WmiByteString($m.SerialNumberID)
        $mfr = Convert-WmiByteString($m.ManufacturerName)
        $year = if ($m.YearOfManufacture) { $m.YearOfManufacture } else { $null }
        $week = if ($m.WeekOfManufacture) { $m.WeekOfManufacture } else { $null }
        $monitors += [ordered]@{
            name = if ($name) { $name.Trim() } else { 'Monitor' }
            manufacturer = if ($mfr) { $mfr.Trim() } else { $null }
            serial = if ($serial) { $serial.Trim() } else { $null }
            product_code = Convert-WmiByteString($m.ProductCodeID)
            manufacture_year = $year; manufacture_week = $week
            instance = $m.InstanceName
            active = $m.Active
        }
    }
} catch {}
if ($monitors.Count -eq 0) {
    $monitors = @(Get-CimInstance Win32_DesktopMonitor -ErrorAction SilentlyContinue | ForEach-Object {
        [ordered]@{
            name = $_.Name; screen_height = $_.ScreenHeight; screen_width = $_.ScreenWidth
            pixels_per_inch = $_.PixelsPerXLogicalInch; status = $_.Status
            manufacturer = $_.MonitorManufacturer; type = $_.MonitorType
        }
    })
}

$sound = @(Get-CimInstance Win32_SoundDevice | ForEach-Object {
    [ordered]@{ name = $_.Name; manufacturer = $_.Manufacturer; status = $_.Status; pnp_device_id = $_.PNPDeviceID }
})

$netPhysical = @(Get-CimInstance Win32_NetworkAdapter | Where-Object { $_.PhysicalAdapter -eq $true } | ForEach-Object {
    [ordered]@{
        name = $_.Name; manufacturer = $_.Manufacturer; mac = $_.MACAddress
        adapter_type = $_.AdapterType; speed = $_.Speed; pnp_device_id = $_.PNPDeviceID
        net_enabled = $_.NetEnabled; status = $_.Status
    }
})

$keyboards = @(Get-CimInstance Win32_Keyboard | ForEach-Object {
    [ordered]@{ name = $_.Name; description = $_.Description; layout = $_.Layout; status = $_.Status; pnp_device_id = $_.PNPDeviceID }
})
$mice = @(Get-CimInstance Win32_PointingDevice | ForEach-Object {
    [ordered]@{
        name = $_.Name; description = $_.Description; manufacturer = $_.Manufacturer
        device_type = $_.DeviceType; hardware_type = $_.HardwareType; status = $_.Status
        pnp_device_id = $_.PNPDeviceID; pointing_type = $_.PointingType
    }
})
# PnP extras para teclado/mouse/HID
$hidInput = @(Get-CimInstance Win32_PnPEntity | Where-Object {
    $_.PNPClass -in @('Keyboard','Mouse','HIDClass') -or $_.Name -match 'keyboard|mouse|teclado|mouse|HID'
} | ForEach-Object {
    [ordered]@{
        name = $_.Name; class = $_.PNPClass; manufacturer = $_.Manufacturer
        status = $_.Status; pnp_device_id = $_.PNPDeviceID; service = $_.Service
    }
})

$usbControllers = @(Get-CimInstance Win32_USBController | ForEach-Object {
    [ordered]@{ name = $_.Name; manufacturer = $_.Manufacturer; status = $_.Status; pnp_device_id = $_.PNPDeviceID }
})

$scsi = @(Get-CimInstance Win32_SCSIController | ForEach-Object {
    [ordered]@{ name = $_.Name; manufacturer = $_.Manufacturer; driver = $_.DriverName }
})

$tpm = $null
try {
    $tpmInfo = Get-CimInstance -Namespace root\CIMV2\Security\MicrosoftTpm -ClassName Win32_Tpm -ErrorAction SilentlyContinue
    if ($tpmInfo) {
        $tpm = [ordered]@{
            present = $true; version = $tpmInfo.SpecVersion
            enabled = $tpmInfo.IsEnabled_InitialValue; activated = $tpmInfo.IsActivated_InitialValue
            owned = $tpmInfo.IsOwned_InitialValue
        }
    }
} catch {}

$battery = @()
Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | ForEach-Object {
    $battery += [ordered]@{
        name = $_.Name; chemistry = $_.Chemistry; design_capacity = $_.DesignCapacity
        full_charge_capacity = $_.FullChargeCapacity; estimated_charge = $_.EstimatedChargeRemaining
        status = $_.BatteryStatus; voltage = $_.Voltage
    }
}

$secureBoot = $null
try {
    $sb = Confirm-SecureBootUEFI -ErrorAction SilentlyContinue
    $secureBoot = @{ enabled = $sb }
} catch {
    $secureBoot = @{ enabled = $null; note = 'Nao disponivel ou Legacy BIOS' }
}

$result = [ordered]@{
    system = [ordered]@{
        manufacturer   = $cs.Manufacturer
        model          = $cs.Model
        system_family  = $cs.SystemFamily
        system_sku     = $cs.SystemSKUNumber
        system_type    = $cs.SystemType
        total_physical_memory_gb = [math]::Round($cs.TotalPhysicalMemory / 1GB, 2)
        domain         = $cs.Domain
        workgroup      = $cs.Workgroup
        uuid           = $prod.UUID
        identifying_number = $prod.IdentifyingNumber
        product_name   = $prod.Name
        vendor         = $prod.Vendor
        version        = $prod.Version
    }
    chassis = [ordered]@{
        type_code   = $enclosure.ChassisTypes[0]
        type_name   = Get-ChassisTypeName $enclosure.ChassisTypes[0]
        manufacturer = $enclosure.Manufacturer
        serial      = $enclosure.SerialNumber
        smbios_tag  = $enclosure.SMBIOSAssetTag
        lock_present = $enclosure.LockPresent
    }
    motherboard = [ordered]@{
        manufacturer = $board.Manufacturer
        product      = $board.Product
        serial       = $board.SerialNumber
        version      = $board.Version
        part_number  = $board.PartNumber
        model        = $board.Model
        status       = $board.Status
    }
    bios = [ordered]@{
        manufacturer  = $bios.Manufacturer
        name          = $bios.Name
        version       = $bios.SMBIOSBIOSVersion
        release_date  = $bios.ReleaseDate
        serial        = $bios.SerialNumber
        smbios_version = $bios.SMBIOSMajorVersion.ToString() + '.' + $bios.SMBIOSMinorVersion.ToString()
        firmware_type = if ($bios.BIOSVersion -match 'UEFI') { 'UEFI' } else { 'BIOS/UEFI' }
    }
    cpu              = $cpus
    ram              = [ordered]@{ total_gb = [math]::Round($ramTotal, 2); modules = $memModules; slot_count = $memModules.Count }
    logical_disks    = $logicalDisks
    physical_disks   = $physicalDisks
    gpu              = $gpus
    monitors         = $monitors
    sound_devices    = $sound
    network_adapters = $netPhysical
    input_devices    = [ordered]@{
        keyboards = $keyboards
        mice      = $mice
        hid_pnp   = $hidInput
    }
    usb_controllers  = $usbControllers
    scsi_controllers = $scsi
    tpm              = $tpm
    battery          = $battery
    secure_boot      = $secureBoot
    temperatures     = @(
        try {
            Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop |
                ForEach-Object {
                    $c = if ($_.CurrentTemperature) { [math]::Round(($_.CurrentTemperature / 10) - 273.15, 1) } else { $null }
                    [ordered]@{ source = 'ThermalZone'; celsius = $c; instance = $_.InstanceName }
                }
        } catch { @() }
    )
}

$result | ConvertTo-Json -Compress -Depth 10
