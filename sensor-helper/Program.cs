using System.Text.Json;
using LibreHardwareMonitor.Hardware;
using LibreHardwareMonitor.PawnIo;

sealed record TemperatureSensor(string Hardware, string Name, double ValueC);
sealed record SensorResponse(List<TemperatureSensor> Sensors, List<string> Hardware, List<string> SensorInventory, bool PawnIoInstalled, string? PawnIoVersion, string? Error);

sealed class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer) => computer.Traverse(this);
    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (var subHardware in hardware.SubHardware)
            subHardware.Accept(this);
    }
    public void VisitSensor(ISensor sensor) { }
    public void VisitParameter(IParameter parameter) { }
}

static class Program
{
    static readonly JsonSerializerOptions JsonOptions = new()
    {
        PropertyNamingPolicy = JsonNamingPolicy.CamelCase
    };

    static SensorResponse ReadSensors(Computer computer, UpdateVisitor visitor)
    {
        try
        {
            computer.Accept(visitor);
            var sensors = new List<TemperatureSensor>();
            foreach (var hardware in computer.Hardware)
                Collect(hardware, sensors);
            var hardwareNames = new List<string>();
            var inventory = new List<string>();
            foreach (var hardware in computer.Hardware)
            {
                hardwareNames.Add($"{hardware.HardwareType}: {hardware.Name}");
                CollectInventory(hardware, inventory);
            }
            return new SensorResponse(sensors, hardwareNames, inventory, PawnIo.IsInstalled, PawnIo.Version?.ToString(), null);
        }
        catch (Exception ex)
        {
            return new SensorResponse(new List<TemperatureSensor>(), new List<string>(), new List<string>(), PawnIo.IsInstalled, PawnIo.Version?.ToString(), ex.Message);
        }
    }

    static void CollectInventory(IHardware hardware, List<string> inventory)
    {
        foreach (var sensor in hardware.Sensors)
        {
            var value = sensor.Value is null ? "null" : sensor.Value.Value.ToString("0.###");
            inventory.Add($"{hardware.Name} | {sensor.SensorType} | {sensor.Name} | {value}");
        }
        foreach (var subHardware in hardware.SubHardware)
            CollectInventory(subHardware, inventory);
    }

    static void Collect(IHardware hardware, List<TemperatureSensor> sensors)
    {
        foreach (var sensor in hardware.Sensors)
        {
            if (sensor.SensorType != SensorType.Temperature || sensor.Value is null)
                continue;
            var value = (double)sensor.Value.Value;
            if (double.IsFinite(value) && value >= -20.0 && value <= 150.0)
                sensors.Add(new TemperatureSensor(hardware.Name, sensor.Name, value));
        }
        foreach (var subHardware in hardware.SubHardware)
            Collect(subHardware, sensors);
    }

    static int Main()
    {
        var computer = new Computer { IsCpuEnabled = true };
        try
        {
            computer.Open();
        }
        catch (Exception ex)
        {
            Console.WriteLine(JsonSerializer.Serialize(new SensorResponse(new(), new(), new(), PawnIo.IsInstalled, PawnIo.Version?.ToString(), ex.Message), JsonOptions));
            Console.Out.Flush();
            return 2;
        }

        var visitor = new UpdateVisitor();
        string? command;
        while ((command = Console.ReadLine()) is not null)
        {
            if (command.Equals("quit", StringComparison.OrdinalIgnoreCase))
                break;
            if (!command.Equals("sample", StringComparison.OrdinalIgnoreCase))
                continue;
            Console.WriteLine(JsonSerializer.Serialize(ReadSensors(computer, visitor), JsonOptions));
            Console.Out.Flush();
        }
        computer.Close();
        return 0;
    }
}
